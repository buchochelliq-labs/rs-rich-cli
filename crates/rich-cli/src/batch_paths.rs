//! Filename-template grammar and validated batch destination planning.

use std::ffi::{OsStr, OsString};
#[derive(Clone, Debug)]
enum Token {
    Literal(String),
    Stem,
    InputExt,
    OutputExt,
    Index,
}
#[derive(Clone, Debug)]
pub(super) struct FilenameTemplate(Vec<Token>);
pub(super) struct FilenameTokens<'a> {
    pub stem: &'a OsStr,
    pub input_ext: &'a OsStr,
    pub output_ext: &'a str,
    pub index: usize,
}
#[derive(Debug)]
pub(super) enum PathPlanError {
    InvalidTemplate(String),
    InvalidLeaf(String),
}
impl std::fmt::Display for PathPlanError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::InvalidTemplate(s) => write!(f, "invalid batch template: {s}"),
            Self::InvalidLeaf(s) => write!(f, "invalid batch filename: {s}"),
        }
    }
}
impl std::error::Error for PathPlanError {}
impl FilenameTemplate {
    pub fn parse(source: &str) -> Result<Self, PathPlanError> {
        let mut chars = source.chars().peekable();
        let mut tokens = Vec::new();
        let mut literal = String::new();
        while let Some(c) = chars.next() {
            if c == '{' || c == '}' {
                if chars.peek() == Some(&c) {
                    chars.next();
                    literal.push(c);
                    continue;
                }
                if c == '}' {
                    return Err(PathPlanError::InvalidTemplate(
                        "unmatched closing brace".into(),
                    ));
                }
                if !literal.is_empty() {
                    tokens.push(Token::Literal(std::mem::take(&mut literal)));
                }
                let mut name = String::new();
                loop {
                    match chars.next() {
                        Some('}') => break,
                        Some(c) => name.push(c),
                        None => {
                            return Err(PathPlanError::InvalidTemplate("unclosed token".into()))
                        }
                    }
                }
                tokens.push(match name.as_str() {
                    "stem" => Token::Stem,
                    "input_ext" => Token::InputExt,
                    "output_ext" => Token::OutputExt,
                    "index" => Token::Index,
                    _ => return Err(PathPlanError::InvalidTemplate(name)),
                });
            } else {
                literal.push(c);
            }
        }
        if !literal.is_empty() {
            tokens.push(Token::Literal(literal));
        }
        Ok(Self(tokens))
    }
    pub fn expand(&self, args: &FilenameTokens<'_>) -> Result<OsString, PathPlanError> {
        if args.index == 0 {
            return Err(PathPlanError::InvalidTemplate("index is one-based".into()));
        }
        let mut leaf = OsString::new();
        for token in &self.0 {
            match token {
                Token::Literal(s) => leaf.push(s),
                Token::Stem => leaf.push(args.stem),
                Token::InputExt => leaf.push(args.input_ext),
                Token::OutputExt => leaf.push(args.output_ext),
                Token::Index => leaf.push(args.index.to_string()),
            }
        }
        let bytes = leaf.as_encoded_bytes();
        if leaf.is_empty()
            || leaf == "."
            || leaf == ".."
            || bytes.iter().any(|b| matches!(*b, b'/' | b'\\' | b':' | 0))
        {
            return Err(PathPlanError::InvalidLeaf(leaf.to_string_lossy().into()));
        }
        #[cfg(windows)]
        {
            let text = leaf.to_string_lossy();
            let base = text.split('.').next().unwrap_or("").to_ascii_uppercase();
            if text.ends_with(['.', ' '])
                || text
                    .chars()
                    .any(|c| c < ' ' || matches!(c, '<' | '>' | '"' | '|' | '?' | '*'))
                || matches!(base.as_str(), "CON" | "PRN" | "AUX" | "NUL")
                || (base.len() == 4
                    && (base.starts_with("COM") || base.starts_with("LPT"))
                    && matches!(base.as_bytes()[3], b'1'..=b'9'))
            {
                return Err(PathPlanError::InvalidLeaf(text.into()));
            }
        }
        Ok(leaf)
    }
}

#[derive(Clone, Debug, Default)]
pub(super) struct BatchPathOptions {
    pub preserve_dirs: bool,
    pub input_root: Option<std::path::PathBuf>,
    pub template: Option<FilenameTemplate>,
}
pub(super) struct PlannedDestination {
    pub path: std::path::PathBuf,
    pub create_parents: Vec<std::path::PathBuf>,
}
fn resolved(path: &std::path::Path) -> Result<std::path::PathBuf, PathPlanError> {
    let absolute = if path.is_absolute() {
        path.to_owned()
    } else {
        std::env::current_dir()
            .map_err(|e| PathPlanError::InvalidLeaf(e.to_string()))?
            .join(path)
    };
    let mut ancestor = absolute.clone();
    let mut suffix = Vec::new();
    while !ancestor.exists() {
        if ancestor.symlink_metadata().is_ok() {
            return Err(PathPlanError::InvalidLeaf(format!(
                "dangling path: {}",
                ancestor.display()
            )));
        }
        suffix.push(
            ancestor
                .file_name()
                .ok_or_else(|| PathPlanError::InvalidLeaf(absolute.display().to_string()))?
                .to_owned(),
        );
        ancestor.pop();
    }
    if !suffix.is_empty() && !ancestor.is_dir() {
        return Err(PathPlanError::InvalidLeaf(format!(
            "output parent is not a directory: {}",
            ancestor.display()
        )));
    }
    let mut result = ancestor
        .canonicalize()
        .map_err(|e| PathPlanError::InvalidLeaf(e.to_string()))?;
    for part in suffix.into_iter().rev() {
        result.push(part);
    }
    Ok(result)
}
pub(super) fn plan_destination(
    input: &std::path::Path,
    output_root: &std::path::Path,
    output_ext: &str,
    index: usize,
    options: &BatchPathOptions,
) -> Result<PlannedDestination, PathPlanError> {
    use std::path::Path;
    let canonical_root = resolved(output_root)?;
    if output_root.exists() && !output_root.is_dir() {
        return Err(PathPlanError::InvalidLeaf(
            "output root is not a directory".into(),
        ));
    }
    let relative = if options.preserve_dirs {
        let root = options
            .input_root
            .as_ref()
            .ok_or_else(|| PathPlanError::InvalidLeaf("missing input root".into()))?
            .canonicalize()
            .map_err(|e| PathPlanError::InvalidLeaf(e.to_string()))?;
        input
            .canonicalize()
            .map_err(|e| PathPlanError::InvalidLeaf(e.to_string()))?
            .strip_prefix(&root)
            .map_err(|_| PathPlanError::InvalidLeaf("input is outside input root".into()))?
            .to_owned()
    } else {
        input
            .file_name()
            .map(std::path::PathBuf::from)
            .ok_or_else(|| PathPlanError::InvalidLeaf("missing input filename".into()))?
    };
    let default_template = FilenameTemplate::parse("{stem}.{output_ext}")?;
    let leaf = options
        .template
        .as_ref()
        .unwrap_or(&default_template)
        .expand(&FilenameTokens {
            stem: input.file_stem().unwrap_or_else(|| OsStr::new("output")),
            input_ext: input.extension().unwrap_or_else(|| OsStr::new("")),
            output_ext,
            index,
        })?;
    let parent = if options.preserve_dirs {
        relative.parent().unwrap_or_else(|| Path::new(""))
    } else {
        Path::new("")
    };
    let path = output_root.join(parent).join(leaf);
    if !resolved(&path)?.starts_with(&canonical_root) {
        return Err(PathPlanError::InvalidLeaf(
            "destination is outside output root".into(),
        ));
    }
    let mut create_parents = Vec::new();
    let mut parent = path.parent();
    while let Some(p) = parent.filter(|p| !p.as_os_str().is_empty()) {
        if p.exists() {
            if !p.is_dir() {
                return Err(PathPlanError::InvalidLeaf(
                    "output parent is not a directory".into(),
                ));
            }
            break;
        }
        create_parents.push(p.to_owned());
        parent = p.parent();
    }
    create_parents.reverse();
    Ok(PlannedDestination {
        path,
        create_parents,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::ffi::{OsStr, OsString};
    #[test]
    fn complete_leaf_tokens_escaping_and_validation() {
        let args = FilenameTokens {
            stem: OsStr::new("report"),
            input_ext: OsStr::new("txt"),
            output_ext: "html",
            index: 1,
        };
        assert_eq!(
            FilenameTemplate::parse("{index}-{stem}.{output_ext}")
                .unwrap()
                .expand(&args)
                .unwrap(),
            OsString::from("1-report.html")
        );
        assert_eq!(
            FilenameTemplate::parse("{stem}.{{draft}}.{output_ext}")
                .unwrap()
                .expand(&args)
                .unwrap(),
            OsString::from("report.{draft}.html")
        );
        for value in ["{unknown}", "{stem", "stem}"] {
            assert!(FilenameTemplate::parse(value).is_err());
        }
        for value in ["", ".", "..", "a/b", "a\\b", "C:escape", "bad\0name"] {
            assert!(
                FilenameTemplate::parse(value)
                    .and_then(|t| t.expand(&args))
                    .is_err(),
                "{value:?}"
            );
        }
    }
    #[cfg(unix)]
    #[test]
    fn os_tokens_preserve_non_utf8_bytes() {
        use std::os::unix::ffi::{OsStrExt, OsStringExt};
        let args = FilenameTokens {
            stem: OsStr::from_bytes(b"file\xff"),
            input_ext: OsStr::new(""),
            output_ext: "svg",
            index: 3,
        };
        assert_eq!(
            FilenameTemplate::parse("{stem}.{output_ext}")
                .unwrap()
                .expand(&args)
                .unwrap()
                .into_vec(),
            b"file\xff.svg"
        );
    }
}
