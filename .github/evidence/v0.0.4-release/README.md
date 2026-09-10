# 0.0.4 source preparation verification

This prepares all four independently selected package versions at 0.0.4. It
creates no release tag and uploads no package. See the preparation PR for the
package dry-run and exact CI results; publication remains a separate action.

The integrated source passed these gates with Rust 1.98.1 on Linux:

```bash
cargo fmt --all --check
cargo clippy --all-targets -- -D warnings
cargo test --all
cargo clippy --all-targets --features syntax-cache -- -D warnings
cargo test --all --features syntax-cache
cargo check --workspace --locked
cargo build -p rs-rich-cli --locked
python3 scripts/gen_cli_reference.py --binary target/debug/rich --check
python3 scripts/test_cli_terminal.py --binary target/debug/rich
python3 scripts/gen_versions.py --check
python3 -m unittest discover -s scripts -p 'test_release*.py' -v
env -u NO_COLOR TERM=xterm-256color PYTHONUTF8=1 python scripts/capture_golden.py
git diff --exit-code -- crates/rich/tests/golden
python3 -m mkdocs build --strict
```

The library oracle is isolated and pinned to Rich 15.0.0. The 22 release
regressions pass. `selection-plans.json` records the actual coordinated,
CLI-only and art-only selection output from the prepared manifests.

Independent sub-agent review found no blockers in manifest/dependency/version
coherence, opt-in syntax behavior, measurements or publication wording.

Actual CLI screenshots and raw implementation evidence are in the neighboring
`v0.0.4-diagnostics`, `v0.0.4-gif`, `v0.0.4-csv`, `v0.0.4-wrapping` and
`v0.0.4-syntax` evidence directories, and the release page embeds the product
screenshots. The syntax feature is off by default; its revised binary reproduces
the retained CLI stdout and native SVG byte for byte.
