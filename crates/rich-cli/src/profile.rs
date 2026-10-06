//! `rich profile`: a profile of CSV, TSV or JSON Lines (0.0.16 workstream 3,
//! #343-#346). Not upstream: a binary-boundary convenience that composes
//! `rs-rich-data`'s `profile`; see `docs/PORTING.md`.
//!
//! This file is command routing: choosing the reader (by extension, else by
//! the first character, as `rich chart` chooses), streaming a file or stdin
//! into it so a long input is sampled rather than read into memory, and
//! `--report json`. The profile itself is `rich_data::profile`'s.
use super::*;

use std::io::BufReader;

use rich_data::csv::{CsvReader, Header};
use rich_data::profile::{Profile, ProfileOptions as DataProfileOptions};
use rich_data::{jsonl, RowSource};

/// The options of `rich profile`.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct ProfileOptions {
    /// `--sample N`: the most rows kept for types and statistics.
    sample: Option<usize>,
    /// `--columns a,b`: only these columns, in this order.
    columns: Option<Vec<String>>,
    /// `--top N`: the most common values shown per categorical column.
    top: Option<usize>,
}

fn count(flag: &str, value: Option<&String>) -> Result<usize, String> {
    value
        .and_then(|v| v.trim().replace('_', "").parse::<usize>().ok())
        .filter(|n| *n > 0)
        .ok_or_else(|| format!("{flag} requires a positive integer"))
}

impl ProfileOptions {
    /// Consume one of these options; anything else stays with the main parser.
    pub(crate) fn parse_option<'a>(
        &mut self,
        arg: &str,
        rest: &mut impl Iterator<Item = &'a String>,
    ) -> Result<bool, String> {
        match arg {
            "--sample" => self.sample = Some(count(arg, rest.next())?),
            "--top" => self.top = Some(count(arg, rest.next())?),
            "--columns" => {
                let value = rest.next().ok_or("--columns requires COLUMN[,COLUMN...]")?;
                let columns: Vec<String> = value
                    .split(',')
                    .map(|c| c.trim().to_string())
                    .filter(|c| !c.is_empty())
                    .collect();
                if columns.is_empty() {
                    return Err("--columns requires COLUMN[,COLUMN...]".into());
                }
                self.columns.get_or_insert_with(Vec::new).extend(columns);
            }
            _ => return Ok(false),
        }
        Ok(true)
    }

    /// Each option given with the commands it applies to, for the "only has
    /// an effect" check.
    pub(crate) fn given(&self) -> Vec<(&'static str, &'static [&'static str])> {
        [
            ("--sample", self.sample.is_some()),
            ("--columns", self.columns.is_some()),
            ("--top", self.top.is_some()),
        ]
        .into_iter()
        .filter(|(_, given)| *given)
        .map(|(flag, _)| (flag, &["profile"][..]))
        .collect()
    }

    fn data_options(&self) -> DataProfileOptions {
        let defaults = DataProfileOptions::default();
        DataProfileOptions {
            sample: self.sample.unwrap_or(defaults.sample),
            top: self.top.unwrap_or(defaults.top),
            columns: self.columns.clone(),
            ..defaults
        }
    }
}

type Failure = (ExitClass, String);

/// How the input is read.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Format {
    /// CSV or TSV, sniffed; the delimiter to fall back to.
    Csv(Option<char>),
    JsonLines,
}

/// The format by the resource's extension, else `None`.
fn format_by_name(resource: &str) -> Option<Format> {
    let path = resource.split(['?', '#']).next().unwrap_or(resource);
    let extension = path
        .rsplit(['/', '\\'])
        .next()
        .and_then(|name| name.rsplit_once('.'))
        .map(|(_, ext)| ext.to_ascii_lowercase());
    match extension.as_deref() {
        Some("jsonl" | "ndjson") => Some(Format::JsonLines),
        Some("csv") => Some(Format::Csv(Some(','))),
        Some("tsv" | "tab") => Some(Format::Csv(Some('\t'))),
        _ => None,
    }
}

/// `rich profile [FILE]`: stream the input into a profile.
pub(crate) fn profile(cli: &Cli) -> Result<Profile, Failure> {
    let resource = cli.resource.as_deref().unwrap_or("-");
    let name = if resource == "-" { "<stdin>" } else { resource };
    let input_error =
        |err: std::io::Error| (ExitClass::Input, format!("cannot read {name}: {err}"));
    // Files and stdin stream; a URL's body (already bounded by the fetch
    // limit) and text in another `--encoding` are decoded whole first.
    let mut reader: Box<dyn BufRead> = if is_url(resource) {
        let (text, _) =
            fetch_url(resource, cli.extensions.encoding).map_err(|err| (ExitClass::Input, err))?;
        Box::new(std::io::Cursor::new(text.into_bytes()))
    } else if cli.extensions.encoding.is_some() {
        let text = read_resource(Some(resource), cli.extensions.encoding).map_err(input_error)?;
        Box::new(std::io::Cursor::new(text.into_bytes()))
    } else if resource == "-" {
        if std::io::stdin().is_terminal() {
            let eof = if cfg!(windows) {
                "Ctrl-Z then Enter"
            } else {
                "Ctrl-D"
            };
            eprintln!("rich: reading stdin; finish input with {eof}");
        }
        Box::new(BufReader::new(std::io::stdin()))
    } else {
        let path = fs_path(resource);
        if path.is_dir() {
            return Err((
                ExitClass::Input,
                format!("cannot read {name}: is a directory, not a file"),
            ));
        }
        Box::new(BufReader::new(
            std::fs::File::open(path).map_err(input_error)?,
        ))
    };
    let format = match format_by_name(resource) {
        Some(format) => format,
        None => {
            // The first character that is not whitespace or a byte-order mark.
            let head = reader.fill_buf().map_err(input_error)?;
            let head = head.strip_prefix("\u{feff}".as_bytes()).unwrap_or(head);
            match head.iter().find(|b| !b.is_ascii_whitespace()) {
                Some(b'{' | b'[') => Format::JsonLines,
                _ => Format::Csv(None),
            }
        }
    };
    let data_error = |err: rich_data::DataError| (ExitClass::Data, format!("{name}: {err}"));
    let options = cli.profile.data_options();
    let profile = match format {
        Format::JsonLines => {
            let mut source = jsonl::source(reader).map_err(data_error)?;
            let mut profile = run(name, &mut source, options)?;
            let late: Vec<String> = source.unknown_keys().map(controls::shown).collect();
            if !late.is_empty() {
                profile.note(format!(
                    "keys first seen after record {} are not profiled: {}",
                    jsonl::DEFAULT_SAMPLE,
                    late.join(", ")
                ));
            }
            profile
        }
        Format::Csv(fallback) => {
            // As `rich chart`: a comma when the sniffer finds no delimiter
            // (one column, or no input at all).
            let mut source = CsvReader::new()
                .header_mode(Header::UnlessNumeric)
                .fallback(fallback.unwrap_or(','))
                .source(reader)
                .map_err(data_error)?;
            let mut profile = run(name, &mut source, options)?;
            let longer = source.longer_rows();
            if longer > 0 {
                profile.note(format!(
                    "{longer} {} more cells than the header names; the extra cells are not \
                     profiled",
                    if longer == 1 { "row has" } else { "rows have" }
                ));
            }
            profile
        }
    };
    if profile.columns().is_empty() && cli.profile.columns.is_none() {
        return Err((ExitClass::Data, format!("{name}: no rows to profile")));
    }
    Ok(profile.with_name(controls::shown(name)))
}

/// Profile `source`, a column `--columns` names that is not there and a row
/// that cannot be read being data errors.
fn run(
    name: &str,
    source: &mut impl RowSource,
    options: DataProfileOptions,
) -> Result<Profile, Failure> {
    let mut profiler = rich_data::profile::Profiler::new(source.columns(), options)
        .map_err(|err| (ExitClass::Data, format!("{name}: {err}")))?;
    while let Some(row) = source.next_row() {
        profiler.push(row.map_err(|err| (ExitClass::Data, format!("{name}: {err}")))?);
    }
    Ok(profiler.finish())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_format_comes_from_the_extension() {
        assert_eq!(format_by_name("a.CSV"), Some(Format::Csv(Some(','))));
        assert_eq!(format_by_name("dir.x/a.tsv"), Some(Format::Csv(Some('\t'))));
        assert_eq!(
            format_by_name("https://h/e.ndjson?x=1"),
            Some(Format::JsonLines)
        );
        assert_eq!(format_by_name("data"), None);
        assert_eq!(format_by_name("-"), None);
    }

    #[test]
    fn counts_are_positive() {
        let mut options = ProfileOptions::default();
        let args = ["10_000".to_string()];
        assert!(options.parse_option("--sample", &mut args.iter()).unwrap());
        assert_eq!(options.sample, Some(10_000));
        let zero = ["0".to_string()];
        assert!(options.parse_option("--top", &mut zero.iter()).is_err());
        let empty = [",".to_string()];
        assert!(options
            .parse_option("--columns", &mut empty.iter())
            .is_err());
    }
}
