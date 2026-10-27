//! rs-rich-data's JSON Lines readers: the whole-text reader and the
//! streaming source, then type inference over what was read.
#![no_main]

use libfuzzer_sys::fuzz_target;
use rich_data::infer::Inferrer;
use rich_data::{jsonl, RowSource};
use rich_fuzz::{text, MAX_PARSE_INPUT};

fuzz_target!(|data: &[u8]| {
    let bytes = &data[..data.len().min(MAX_PARSE_INPUT)];
    if let Ok(mut rows) = jsonl::read(&text(bytes, MAX_PARSE_INPUT)) {
        let inference = Inferrer::new().infer(&rows);
        inference.apply(&mut rows);
    }
    if let Ok(mut stream) = jsonl::source(bytes) {
        while let Some(row) = stream.next_row() {
            if row.is_err() {
                break;
            }
        }
    }
});
