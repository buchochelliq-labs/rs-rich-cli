# Rust oracles for the Python tests

Rich has no image art or Mermaid, so `tests/test_art.py` and
`tests/test_mermaid.py` compare the bindings with what the Rust crates render
for the same input. Their `EXPECTED` tables come from `art-oracle`, a small
program that reads a JSON list of cases on stdin, renders each with
`rs-rich-art` / `rs-rich-mermaid`, and prints the results as JSON.

The tests do not build or run the oracle; they only read the tables. After a
change to either crate (or to the cases), regenerate and review the diff:

```bash
cargo build --release --manifest-path crates/rich-py/oracles/art-oracle/Cargo.toml \
    --target-dir target/rich-py            # add --features mmdc for the mmdc cases
python crates/rich-py/oracles/fill_expected.py crates/rich-py/tests/test_art.py
python crates/rich-py/oracles/fill_expected.py crates/rich-py/tests/test_mermaid.py
```

`fill_expected.py` imports the test file (it needs `rs_rich` installed) for its
`CASES` and `digest`, and rewrites the block between `# fmt: off` and
`# fmt: on`. `ART_ORACLE` overrides the oracle's path.
