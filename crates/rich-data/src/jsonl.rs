//! JSON Lines: one JSON value per line, as rows.
//!
//! Each non-blank line is a record; an object's keys are columns, and any
//! other value is one `value` column. [`read`] reads text whole, so every
//! record decides the columns and schema; [`source`] streams from a reader,
//! deciding them from the first [`DEFAULT_SAMPLE`] records (see
//! [`RecordSource`] for what happens to keys seen later). A line longer than
//! [`MAX_LINE`] bytes is refused rather than read into memory.
//!
//! ```
//! use rich_data::{jsonl, RowSource, Value};
//!
//! let rows = jsonl::read("{\"name\": \"web\", \"up\": true}\n\n{\"name\": \"db\", \"p99\": 35}\n").unwrap();
//! assert_eq!(rows.columns(), ["name", "up", "p99"]);
//! assert_eq!(rows.rows()[1], [Value::from("db"), Value::Null, Value::Int(35)]);
//! let schema = rows.schema().unwrap();
//! assert_eq!(schema.field("up").unwrap().data_type().to_string(), "boolean");
//! assert!(schema.field("p99").unwrap().is_nullable());
//! ```

use std::io::{BufRead, Read};

use serde_json::Value as Json;

use crate::{DataError, RecordSource, RowSource, Rows};

pub use crate::record::{DEFAULT_SAMPLE, MAX_DEPTH};

/// The longest line read, in bytes (64 MiB).
pub const MAX_LINE: usize = 64 << 20;

/// Read JSON Lines text, sampling every record for the columns and schema.
pub fn read(text: &str) -> Result<Rows, DataError> {
    RecordSource::new(Lines::new(text.as_bytes()), usize::MAX)?.collect_rows()
}

/// Stream JSON Lines from `reader`, sampling the first [`DEFAULT_SAMPLE`]
/// records for the columns and schema.
pub fn source<R: BufRead>(reader: R) -> Result<JsonlSource<R>, DataError> {
    source_with_sample(reader, DEFAULT_SAMPLE)
}

/// Stream JSON Lines from `reader`, sampling the first `sample` records.
pub fn source_with_sample<R: BufRead>(
    reader: R,
    sample: usize,
) -> Result<JsonlSource<R>, DataError> {
    RecordSource::new(Lines::new(reader), sample)
}

/// A streaming JSON Lines [`RowSource`].
pub type JsonlSource<R> = RecordSource<Lines<R>>;

/// The records of a JSON Lines stream, with their line numbers in errors.
#[derive(Debug)]
pub struct Lines<R> {
    reader: R,
    line: usize,
    buffer: Vec<u8>,
    done: bool,
}

impl<R: BufRead> Lines<R> {
    /// Records from `reader`.
    pub fn new(reader: R) -> Self {
        Lines {
            reader,
            line: 0,
            buffer: Vec::new(),
            done: false,
        }
    }
}

impl<R: BufRead> Iterator for Lines<R> {
    type Item = Result<Json, DataError>;

    fn next(&mut self) -> Option<Self::Item> {
        while !self.done {
            self.line += 1;
            self.buffer.clear();
            let read = (&mut self.reader)
                .take(MAX_LINE as u64 + 1)
                .read_until(b'\n', &mut self.buffer);
            match read {
                Err(error) => {
                    self.done = true;
                    return Some(Err(DataError::at(self.line, error.to_string())));
                }
                Ok(0) => self.done = true,
                Ok(_) => {
                    let mut line = self.buffer.as_slice();
                    if line.len() > MAX_LINE && !line.ends_with(b"\n") {
                        self.done = true;
                        return Some(Err(DataError::at(
                            self.line,
                            format!("longer than {MAX_LINE} bytes"),
                        )));
                    }
                    if self.line == 1 {
                        line = line.strip_prefix("\u{feff}".as_bytes()).unwrap_or(line);
                    }
                    if line.iter().all(u8::is_ascii_whitespace) {
                        continue;
                    }
                    return Some(
                        serde_json::from_slice(line)
                            .map_err(|e| DataError::at(self.line, format!("not JSON: {e}"))),
                    );
                }
            }
        }
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn errors_carry_the_line_number() {
        let error = read("{\"a\": 1}\n\n{\"a\": \n").unwrap_err();
        assert_eq!(error.line(), Some(3));
        assert!(error.message().starts_with("not JSON"));
    }

    #[test]
    fn a_stream_samples_ahead_then_yields_rows() {
        let text = "\u{feff}{\"a\": 1}\r\n{\"a\": 2, \"b\": 3}\n[1, 2]\n";
        let mut source = source_with_sample(text.as_bytes(), 1).unwrap();
        assert_eq!(source.columns(), ["a"]);
        let mut count = 0;
        while let Some(row) = source.next_row() {
            row.unwrap();
            count += 1;
        }
        assert_eq!(count, 3);
        assert_eq!(source.unknown_keys().collect::<Vec<_>>(), ["b", "value"]);
    }

    /// A record of many keys, at the top level or nested, is read in time
    /// linear in its keys: each key used to be looked up by scanning every
    /// column, so 40,000 keys (1.7 MB) took 16 s and a 64 MiB line days.
    #[test]
    fn a_record_of_many_keys_reads_in_linear_time() {
        let keys = 200_000;
        let object: String = (0..keys)
            .map(|i| format!("\"k{i}\":{i}"))
            .collect::<Vec<_>>()
            .join(",");
        let text = format!("{{{object}}}\n{{\"nested\":{{{object}}}}}\n{{{object}}}\n");
        let started = std::time::Instant::now();
        let mut source = source(text.as_bytes()).unwrap();
        assert_eq!(source.columns().len(), keys + 1);
        let mut rows = 0;
        while let Some(row) = source.next_row() {
            assert_eq!(row.unwrap().len(), keys + 1);
            rows += 1;
        }
        assert_eq!(rows, 3);
        // Every key is absent from the nested record, so nullable.
        assert!(source.schema().unwrap().fields()[0].is_nullable());
        assert!(
            started.elapsed() < std::time::Duration::from_secs(30),
            "took {:?}",
            started.elapsed()
        );
    }
}
