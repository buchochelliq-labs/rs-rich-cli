//! rs-rich-data's CSV/TSV readers: the sniffer, the whole-text reader and
//! the streaming source, then type inference over what was read. The first
//! byte picks the reader.
#![no_main]

use libfuzzer_sys::fuzz_target;
use rich_data::csv::{has_header, sniff, CsvReader};
use rich_data::infer::Inferrer;
use rich_data::RowSource;
use rich_fuzz::{take_byte, text, MAX_PARSE_INPUT};

fuzz_target!(|data: &[u8]| {
    let (mode, rest) = take_byte(data);
    let bytes = &rest[..rest.len().min(MAX_PARSE_INPUT)];
    let reader = match mode % 3 {
        0 => CsvReader::new(),
        1 => CsvReader::tsv(),
        _ => CsvReader::new().header(mode & 4 == 4),
    };
    let source = text(bytes, MAX_PARSE_INPUT);
    let _ = sniff(&source, None);
    let _ = has_header(&source);
    if let Ok(mut rows) = reader.read(&source) {
        let inference = Inferrer::new().infer(&rows);
        inference.apply(&mut rows);
    }
    if let Ok(mut stream) = reader.source(bytes) {
        while let Some(row) = stream.next_row() {
            if row.is_err() {
                break;
            }
        }
    }
});
