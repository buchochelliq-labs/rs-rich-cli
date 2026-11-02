# {{project-name}}

A sandboxed WASM plugin for [`rich`](https://github.com/buchochelliq-labs/rs-rich-cli),
the Rust port of Python rich. It adds a `title-case` transform, a `todo`
highlighter and a `callout` Markdown fence; replace them with yours in
`src/lib.rs`.

```bash
rustup target add wasm32-unknown-unknown   # once
cargo test                                 # the unit tests, natively
cargo wasm                                 # target/wasm32-unknown-unknown/release/*.wasm

# Try it with rich (`cargo install rs-rich-cli`):
wasm=target/wasm32-unknown-unknown/release/$(echo {{project-name}} | tr - _).wasm
rich plugins info {{project-name}} --plugin "$wasm"
rich --plugin "$wasm" --transform title-case -p "hello there"
rich --plugin "$wasm" -p "TODO: ship it"
printf '```callout\nBack up first.\n```\n' > note.md && rich --plugin "$wasm" -m note.md
```

## Publishing

Tag a version (`git tag v0.1.0 && git push --tags`) and the release
workflow attaches the module to the GitHub release. Then write its
marketplace listing and open a pull request with it on
[rs-rich-marketplace](https://github.com/buchochelliq-labs/rs-rich-marketplace):

```bash
rich plugin publish {{project-name}}.wasm \
  --url https://github.com/OWNER/REPO/releases/download/v0.1.0/{{project-name}}.wasm \
  --source-revision "$(git rev-parse v0.1.0)" \
  --license "MIT OR Apache-2.0" --repository https://github.com/OWNER/REPO
```

Each step, and what the host checks, is in the guide,
[Write a plugin](https://github.com/buchochelliq-labs/rs-rich-cli/blob/main/docs/guide/write-a-plugin.md).

## Versions

- **The package version** is the plugin's: its manifest reads it from
  `Cargo.toml`. A listed version never changes, so bump it for every
  release.
- **The ABI**, `rich-plugin-abi 1.0` in `MANIFEST`, is what `rich` checks:
  a host loads a plugin of its own ABI major (`rich_plugin_api::abi::ABI_MAJOR`)
  and refuses a capability kind it does not know. Your listing's
  `requires.abi` is the same number.
