//! Markup checks shared by the function-like macros.

/// A tag found in markup, as upstream's `RE_TAGS` finds it.
pub struct Tag {
    /// The text between the brackets.
    pub content: String,
    /// Byte range of the whole tag, brackets included.
    pub start: usize,
    pub end: usize,
}

pub fn is_tag_start(c: char) -> bool {
    c.is_ascii_lowercase() || matches!(c, '#' | '/' | '@')
}

/// Every unescaped tag, in order. Port of upstream's
/// `((\\*)\[([a-z#/@][^[]*?)])`: an odd run of backslashes escapes the tag.
pub fn tags(markup: &str) -> Vec<Tag> {
    let bytes = markup.as_bytes();
    let mut out = Vec::new();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] != b'[' {
            i += 1;
            continue;
        }
        let backslashes = markup[..i]
            .bytes()
            .rev()
            .take_while(|b| *b == b'\\')
            .count();
        let starts = markup[i + 1..].chars().next().is_some_and(is_tag_start);
        let close = markup[i + 1..]
            .find([']', '['])
            .filter(|at| markup.as_bytes()[i + 1 + at] == b']');
        match (starts, close) {
            (true, Some(at)) if backslashes % 2 == 0 => {
                out.push(Tag {
                    content: markup[i + 1..i + 1 + at].to_string(),
                    start: i,
                    end: i + 2 + at,
                });
                i += 2 + at;
            }
            _ => i += 1,
        }
    }
    out
}

/// Whether `style` is a style definition or a default theme key; a tag that is
/// neither renders as a no-op, which the macros reject.
pub fn known_style(style: &str, keys: &[String]) -> bool {
    let style = style.trim();
    rich::theme::DEFAULT_STYLES
        .iter()
        .any(|(name, _)| *name == style)
        || keys.iter().any(|key| key == style)
        || rich::Style::parse(style).is_ok()
}

/// Check `markup`, skipping style checks for tags containing `dynamic`.
/// Errors name the tag and say how to fix it.
pub fn check(markup: &str, keys: &[String], dynamic: Option<char>) -> Result<(), String> {
    if let Err(error) = rich::markup::render(markup) {
        return Err(format!("invalid markup: {error}"));
    }
    let mut open: Vec<String> = Vec::new();
    for tag in tags(markup) {
        let content = tag.content.trim();
        if let Some(name) = content.strip_prefix('/') {
            // `render` has already rejected a close with nothing open or a
            // mismatched name.
            if name.trim().is_empty() {
                open.pop();
            } else if let Some(at) = open.iter().rposition(|o| o == name.trim()) {
                open.remove(at);
            }
            continue;
        }
        let name = content
            .split_once('=')
            .map_or(content, |(name, _)| name)
            .trim();
        open.push(name.to_string());
        if content.starts_with('@') || name == "link" {
            continue;
        }
        if dynamic.is_some_and(|marker| content.contains(marker)) {
            continue;
        }
        if !known_style(name, keys) {
            return Err(format!(
                "unknown style or theme key `[{content}]`: it would render unstyled. \
                 Use a style such as `bold red`, a default theme key such as `repr.number`, \
                 or declare custom keys with `keys[\"{name}\"]`"
            ));
        }
    }
    if let Some(name) = open.last() {
        return Err(format!(
            "unclosed tag `[{name}]`: close it with `[/{name}]` or `[/]`"
        ));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn escaped_and_non_tags_are_skipped() {
        let found: Vec<_> = tags(r"a \[bold] [Bold] [1] [b]x[/b] \\[i]y[/i]")
            .into_iter()
            .map(|t| t.content)
            .collect();
        assert_eq!(found, ["b", "/b", "i", "/i"]);
    }

    #[test]
    fn checks_catch_the_silent_failures() {
        let keys = vec!["app.title".to_string()];
        assert!(check("[bold red]x[/]", &keys, None).is_ok());
        assert!(check("[repr.number]1[/repr.number]", &keys, None).is_ok());
        assert!(check("[app.title]t[/]", &keys, None).is_ok());
        assert!(check("[link=https://x.io]x[/link]", &keys, None).is_ok());
        assert!(check("[bodl]x[/]", &keys, None)
            .unwrap_err()
            .contains("unknown style"));
        assert!(check("[bold]x", &keys, None)
            .unwrap_err()
            .contains("unclosed"));
        assert!(check("x[/bold]", &keys, None)
            .unwrap_err()
            .contains("invalid markup"));
        assert!(check("[bold]x[/italic]", &keys, None).is_err());
        assert!(check("[x\u{E000}]x[/]", &keys, Some('\u{E000}')).is_ok());
    }
}
