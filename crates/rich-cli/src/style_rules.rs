//! `--style-rule` and `--rules`: conditional styles for the data tables the
//! CLI draws from a file (0.0.19 workstream 15, #701). Not upstream: a
//! binary-boundary convenience that composes `rs-rich-ext`'s
//! `table::rules`; see `docs/PORTING.md`.
//!
//! A rule is read here and applied by whatever draws the table: `--csv`
//! (and a `.csv` or `.tsv` drawn automatically), `rich profile`'s table of
//! columns, and `rich inspect --table`'s records. Elsewhere rules have no
//! effect, so a config profile's `[[rules]]` never stops another command.
//! The spec grammar is `StyleRule::parse`'s and the rule file is
//! `StyleRules::from_toml`'s.
use std::io::Read;

use rich_ext::table::{ResolvedRules, StyleRule, StyleRules};

/// The largest `--rules` file read (1 MiB): rules are a few lines.
const MAX_RULES_FILE: u64 = 1 << 20;

/// `--style-rule SPEC` (repeatable) and `--rules FILE`, in the order given.
#[derive(Clone, Debug, Default)]
pub(crate) struct RuleOptions {
    rules: StyleRules,
}

impl RuleOptions {
    /// Consume one of these options; anything else stays with the main
    /// parser. A rule that does not parse, or a file that cannot be read or
    /// holds a bad rule table, is a usage error naming it.
    pub(crate) fn parse_option<'a>(
        &mut self,
        arg: &str,
        rest: &mut impl Iterator<Item = &'a String>,
    ) -> Result<bool, String> {
        match arg {
            "--style-rule" => {
                let spec = rest
                    .next()
                    .ok_or("--style-rule requires COLUMN OP VALUE:STYLE")?;
                let rule = StyleRule::parse(spec).map_err(|err| format!("--style-rule: {err}"))?;
                self.rules.push(rule);
            }
            "--rules" => {
                let path = rest.next().ok_or("--rules requires a FILE")?;
                let shown = super::controls::shown(path);
                let text =
                    read_rules_file(path).map_err(|err| format!("--rules {shown}: {err}"))?;
                let rules = StyleRules::from_toml(&text)
                    .map_err(|err| format!("--rules {shown}: {err}"))?;
                for rule in rules.rules() {
                    self.rules.push(rule.clone());
                }
            }
            _ => return Ok(false),
        }
        Ok(true)
    }

    /// The rules, in order.
    pub(crate) fn rules(&self) -> &StyleRules {
        &self.rules
    }

    /// Say on stderr which rules name a column `headers` lacks (they never
    /// match), unless `quiet`.
    pub(crate) fn warn_unresolved(&self, headers: &[&str], quiet: bool) {
        if quiet || self.rules.is_empty() {
            return;
        }
        let resolved = self.rules.resolve(headers, std::iter::empty::<Vec<_>>());
        warn_unresolved(&resolved, headers);
    }
}

/// Say on stderr which of `resolved`'s rules name a column `headers` lacks:
/// they never match, and a typo should not pass for "nothing matched".
pub(crate) fn warn_unresolved(resolved: &ResolvedRules<'_>, headers: &[&str]) {
    for rule in resolved.unresolved() {
        eprintln!(
            "rich: warning: style rule {} names no column of this table (columns: {})",
            super::controls::shown(&format!("{:?}", rule.to_string())),
            headers
                .iter()
                .map(|h| super::controls::shown(h))
                .collect::<Vec<_>>()
                .join(", ")
        );
    }
}

/// A rules file's text: a regular file (not a directory or a FIFO, which
/// would hang), at most [`MAX_RULES_FILE`] bytes of UTF-8.
fn read_rules_file(path: &str) -> Result<String, String> {
    let path = super::fs_path(path);
    let metadata = std::fs::metadata(&path).map_err(|err| err.to_string())?;
    if !metadata.is_file() {
        return Err("not a regular file".into());
    }
    if metadata.len() > MAX_RULES_FILE {
        return Err(format!("larger than {MAX_RULES_FILE} bytes"));
    }
    let mut text = String::new();
    std::fs::File::open(&path)
        .and_then(|file| file.take(MAX_RULES_FILE).read_to_string(&mut text))
        .map_err(|err| err.to_string())?;
    Ok(text)
}

/// The rule specs of a config's `rules` value (an array of rule tables, as
/// `--rules` files hold), for `--style-rule` flags; an error names the
/// table.
pub(crate) fn config_specs(value: &toml::Value) -> Result<Vec<String>, String> {
    let rules = StyleRules::from_toml_value(value).map_err(|err| err.to_string())?;
    rules
        .rules()
        .iter()
        .enumerate()
        .map(|(index, rule)| {
            rule.to_spec().ok_or_else(|| {
                format!("rules[{index}]: a rule a --style-rule spec cannot write (a float that is not finite)")
            })
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parsed(args: &[&str]) -> Result<RuleOptions, String> {
        let args: Vec<String> = args.iter().map(|s| s.to_string()).collect();
        let mut options = RuleOptions::default();
        let mut iter = args.iter();
        while let Some(arg) = iter.next() {
            assert!(options.parse_option(arg, &mut iter)?, "{arg} not consumed");
        }
        Ok(options)
    }

    #[test]
    fn specs_and_files_add_rules_in_order() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("rules.toml");
        std::fs::write(
            &file,
            "[[rules]]\ncolumn = 'status'\nvalue = 'failed'\nstyle = 'red'\ntarget = 'row'\n",
        )
        .unwrap();
        let options = parsed(&[
            "--style-rule",
            "latency>500:bold",
            "--rules",
            file.to_str().unwrap(),
            "--style-rule",
            "note empty:dim",
        ])
        .unwrap();
        let specs: Vec<String> = options
            .rules()
            .rules()
            .iter()
            .map(|r| r.to_string())
            .collect();
        assert_eq!(
            specs,
            [
                "latency>500:bold",
                "status==failed:row:red",
                "note empty:dim"
            ]
        );
    }

    #[test]
    fn bad_rules_name_the_flag_and_the_rule() {
        let error = parsed(&["--style-rule", "latency>:red"]).unwrap_err();
        assert!(
            error.starts_with("--style-rule: rule \"latency>:red\": no value"),
            "{error}"
        );
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("bad.toml");
        std::fs::write(
            &file,
            "[[rules]]\ncolumn = 'a'\nop = 'like'\nvalue = 1\nstyle = 'red'\n",
        )
        .unwrap();
        let error = parsed(&["--rules", file.to_str().unwrap()]).unwrap_err();
        assert!(
            error.ends_with("bad.toml: rules[0]: unknown op `like`"),
            "{error}"
        );
        let error = parsed(&["--rules", dir.path().to_str().unwrap()]).unwrap_err();
        assert!(error.ends_with("not a regular file"), "{error}");
        let error = parsed(&["--rules", "missing.toml"]).unwrap_err();
        assert!(error.starts_with("--rules missing.toml: "), "{error}");
        assert!(parsed(&["--style-rule"]).is_err());
    }

    #[test]
    fn config_rule_tables_become_specs() {
        let value: toml::Value = toml::from_str::<toml::Table>(
            "rules = [{column = 'p99 ms', op = '>=', value = 0.5, style = 'yellow'}, \
             {column = 2, op = 'empty', style = 'dim', target = 'row'}]",
        )
        .unwrap()["rules"]
            .clone();
        assert_eq!(
            config_specs(&value).unwrap(),
            ["p99 ms>=0.5:yellow", "#2 empty:row:dim"]
        );
        let bad: toml::Value =
            toml::from_str::<toml::Table>("rules = [{column = 'a'}]").unwrap()["rules"].clone();
        assert_eq!(config_specs(&bad).unwrap_err(), "rules[0]: missing `value`");
    }
}
