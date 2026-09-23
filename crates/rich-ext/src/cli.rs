//! Optional CLI conveniences, kept outside the faithful command parser.

use crate::cli_doc::ArgSpec;
use crate::encoding::Encoding;

/// Options owned by the extension layer; the executable supplies input/mode facts.
#[derive(Default, Clone)]
pub struct CliExtensions {
    pub encoding: Option<Encoding>,
    gif_blocks: Option<bool>,
}

pub const HELP: &str = "        --gif-mode M With --gif: ascii (default) or blocks (half-block pixels).\n                     Blocks fall back to ASCII without color or when piped.\n        --encoding E Explicit text encoding: utf-8, utf-16 (BOM required),\n                     utf-16le or utf-16be. Strict; files, stdin and URLs only.";

/// The extension options as [`ArgSpec`]s, for help, completions and docs.
/// Carries the same text as [`HELP`], which is kept for compatibility.
pub fn arg_specs() -> Vec<ArgSpec> {
    vec![
        ArgSpec::option("gif-mode")
            .value_name("M")
            .choices(["ascii", "blocks"])
            .default_value("ascii")
            .help("With --gif: ascii or blocks (half-block pixels). Blocks fall back to ASCII without color or when piped."),
        ArgSpec::option("encoding")
            .value_name("E")
            .choices(["utf-8", "utf-16", "utf-16le", "utf-16be"])
            .help("Explicit text encoding: utf-8, utf-16 (BOM required), utf-16le or utf-16be. Strict; files, stdin and URLs only."),
    ]
}

impl CliExtensions {
    /// Consume an extension option. Unknown options remain with the mirror parser.
    pub fn parse_option<'a>(
        &mut self,
        arg: &str,
        rest: &mut impl Iterator<Item = &'a String>,
    ) -> Result<bool, String> {
        match arg {
            "--gif-mode" => {
                self.gif_blocks = Some(match rest.next().map(String::as_str) {
                    Some("ascii") => false,
                    Some("blocks") => true,
                    _ => return Err("--gif-mode requires ascii or blocks".into()),
                })
            }
            "--encoding" => {
                self.encoding = Some(
                    rest.next()
                        .ok_or("--encoding requires an encoding name")?
                        .parse()?,
                )
            }
            _ => return Ok(false),
        }
        Ok(true)
    }

    pub fn validate(&self, gif: bool, reads_text: bool) -> Result<(), String> {
        if self.gif_blocks.is_some() && !gif {
            return Err("--gif-mode only has an effect with --gif".into());
        }
        if self.encoding.is_some() && !reads_text {
            return Err("--encoding requires file, stdin or URL text input".into());
        }
        Ok(())
    }

    /// GIFs retain ASCII defaults unless the extension explicitly requests blocks.
    pub fn gif_blocks(&self) -> bool {
        self.gif_blocks.unwrap_or(false)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn arg_specs_match_the_options_parse_option_consumes() {
        for spec in arg_specs() {
            let long = spec.primary();
            assert!(HELP.contains(&long), "{long} missing from HELP");
            let value = spec.choice_list()[0].value.clone();
            let rest = [value];
            let mut iter = rest.iter();
            assert_eq!(
                CliExtensions::default().parse_option(&long, &mut iter),
                Ok(true),
                "{long}"
            );
        }
    }
}
