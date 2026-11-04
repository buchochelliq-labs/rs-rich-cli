# The 0.1.0 API review: the groundwork

**Status:** groundwork, from [0.0.25's workstream 5](../plans/0.0.25.md#5-bookkeeping-and-a-head-start-on-010-docs-all-crates)
(2026-11-04). The review itself is
[stage 3 of the plan to 1.0.0](../plans/1.0.0.md#stage-3-api-review-and-stability-tooling-010);
this note is two of its lists, made ahead of it: what each stable crate
still lacks in documentation, and every foreign type in the stable
set's public APIs. Nothing here changes an API; the review decides.

The stable set is the sixteen packages in
[What 1.0.0 covers](../plans/1.0.0.md#what-100-covers). Fifteen are Rust
crates in the workspace; the sixteenth, `rs-rich` on PyPI, has no Rust API
of its own and is not counted here.

## Missing documentation

Stage 3 sets `missing_docs` to deny on the stable set. 0.0.25 turns it on
as a warning where a crate is already at zero, and records the count
where it is not. CI runs clippy with `-D warnings`, so a crate gets
`#![warn(missing_docs)]` only once every public item is documented,
features on; until then its count is here instead.

Counted on 2026-11-04 with `cargo rustc -p <crate> --lib --profile=check
-- -W missing_docs` (Rust 1.99), with default features and with all of
them (rs-rich-art's count without `avif`, which adds no item). The after
column is 0.0.25's, with the gaps it filled; every crate with the lint
on passes `cargo clippy --all-targets --all-features -- -D warnings`
(rs-rich-cli: its library, whose two public items no feature changes).

| crate | default features | all features | after 0.0.25 | lint |
|---|---:|---:|---:|---|
| `rs-rich` | 222 | 222 | 222 | not on: see below |
| `rs-rich-plugin-api` | 58 | 58 | 0 | `warn` |
| `rs-rich-macros` | 0 | 0 | 0 | `warn` |
| `rs-rich-ext` | 746 | 1,153 | 1,153 | not yet |
| `rs-rich-cli` | 0 | 0 | 0 | `warn` (its library's public items are `run` and `run_embedded`) |
| `rs-rich-marketplace` | 103 | 103 | 0 | `warn` |
| `rs-rich-art` | 5 | 63 | 0 | `warn` |
| `rs-rich-micro` | 100 | 100 | 0 | `warn` |
| `rs-rich-record` | 152 | 152 | 0 | `warn` |
| `rs-rich-interact` | 323 | 361 | 361 | not yet |
| `rs-rich-intuituive` | 316 | 337 | 337 | not yet |
| `rs-rich-embed` | 28 | 33 | 0 | `warn` |
| `noctiluca` | 301 | 301 | 301 | not yet |
| `rs-rich-acp` | 0 | 0 | 0 | `warn` |
| `cornacai` | 0 | 0 | 0 | `warn` |

What is left, by kind and by file (all features):

| crate | mostly | most in |
|---|---|---|
| `rs-rich` | 113 struct fields, 38 variants, 33 methods, 22 associated functions, 16 constants | `box.rs` 45, `protocol.rs` 33, `console.rs` 33, `pretty.rs` 15, `progress.rs` 12 |
| `rs-rich-ext` | 565 struct fields, 232 variants, 216 methods, 68 associated functions, 43 constants, 13 structs | `qa/fuzz.rs` 62, `event.rs` 55, `document/model.rs` 52, `cli_doc/spec.rs` 50, `palette/material.rs` 43 |
| `rs-rich-interact` | 141 methods, 84 struct fields, 73 associated functions, 58 variants | `event.rs` 46, `compose.rs` 32, `host.rs` 24, `edit.rs` 21, `overlay.rs` 19 |
| `rs-rich-intuituive` | 127 variants, 122 struct fields, 52 methods, 18 associated functions, 17 associated constants | `app.rs` 44, `border.rs` 40, `a11y.rs` 23, `sprite.rs` 18, `datetime.rs` 14 |
| `noctiluca` | 211 struct fields, 49 variants, 29 methods, 11 associated functions | `model.rs` 77, `engine.rs` 47, `tab.rs` 37, `config.rs` 32, `args.rs` 16 |

**Core.** `crates/rich` is the faithful port: its doc comments follow
upstream's docstrings, and [AGENTS.md](https://github.com/buchochelliq-labs/rs-rich-cli/blob/main/AGENTS.md) keeps changes to
it to what an upstream sync brings. A crate-level lint attribute changes
no behaviour, but documenting 222 items that upstream leaves bare is a
port question, so 0.0.25 only counts them; the review decides how core
meets "every public item is documented" (writing them, or allowing
the lint on the items upstream has no docstring for).

Ten of the fifteen are at zero with the lint on. For 0.1.0, the order
that costs least: noctiluca, rs-rich-intuituive and rs-rich-interact
(three hundred or so each, mostly fields and variants), then
rs-rich-ext, the largest by far, and core once the review has decided
how.

## Foreign types in the public APIs

Stage 3's public-dependency review goes through this list: for each
type, hide it behind one of ours, or keep it behind a feature documented
as following that dependency's major version.

Read from the API snapshots (`api/<crate>.txt`, made with every feature
on except for rs-rich-cli, whose snapshot is its default build) at
`3514d742`. A foreign type is any path whose crate is not `std`, `core`,
`alloc` or a workspace crate. Versions are those in `Cargo.lock`. "In the
API" says when the type is there: always, or only with the features
named (the features that pull the dependency in); for a type that comes
through another crate, which one. Methods on primitive types (rs-rich-ext's
`str::accessible_text`, rs-rich-intuituive's `f64::stepped`) are our
extension traits' impls, not foreign types, and are left out.

What stands out:

- **Core** exposes two: `compact_str::CompactString` (`Segment::text`,
  `Color::name`, and its re-export, decided in 0.0.23) and
  `serde_json::Value` (`Json::from_data`, `PrettyValue::from`). `syntect`
  and `pulldown-cmark`, which the plan expected to be the main cases, are
  not in core's public API: the snapshot names neither.
- **Whole crates re-exported:** `rich_art::image` (`pub use image`, which
  rs-rich-micro and rs-rich-intuituive reach `image`'s types through) and
  `rich_marketplace::semver`.
- **serde:** rs-rich-acp (about 75 types) and rs-rich-ext (about 50)
  implement `Serialize` or `Deserialize`; the snapshots name the traits
  by the crate that defines them, `serde_core`, which `serde` re-exports.
  `serde_json::Value` is in seven of the fifteen crates.
- **Behind a feature already:** rs-rich-ext's `anyhow`, `log`, `tracing`,
  `toml` and `clap` adapters, rs-rich-interact's `termion` and `termwiz`
  converters, rs-rich-marketplace's `ureq` (its `http` feature), and
  rs-rich-embed's `serde_json` (`chrome`).
- **Always on:** rs-rich-interact's `crossterm::event::Event`,
  rs-rich-embed's and rs-rich-record's `vt100` screens and mouse modes,
  rs-rich-embed's `portable_pty::ExitStatus` conversion, rs-rich-record's
  `fancy_regex::Regex` in tape masks, and rs-rich-plugin-api's
  `inventory::Collect`, which `export_plugin!` needs.
- **None:** rs-rich-macros, rs-rich-cli and cornacai.

### By crate

#### rs-rich

| foreign crate | version | in the API | type | items |
|---|---|---|---|---|
| `compact_str` | 0.9.1 | always | `compact_str::CompactString` | `Color::name`, `Segment::text`, `color::Color::name`, `segment::Segment::control`, and 2 more |
| `serde_json` | 1.0.154 | always | `serde_json::value::Value` | `impl From for pretty::PrettyValue`, `json::Json::from_data`, `pretty::PrettyValue::from` |

#### rs-rich-plugin-api

| foreign crate | version | in the API | type | items |
|---|---|---|---|---|
| `inventory` | 0.3.25 | always | `inventory::Collect` | `impl Collect for LinkedPlugin` |

#### rs-rich-macros

No foreign type.

#### rs-rich-ext

| foreign crate | version | in the API | type | items |
|---|---|---|---|---|
| `anyhow` | 1.0.104 | feature `anyhow` | `anyhow::Error` | `diagnostic::Diagnostic::from_anyhow` |
| `clap_builder` | 4.6.7 | `clap` | `clap_builder::Error` | `cli_doc::CliError::from_clap`, `cli_doc::clap::ParseExit::error` |
| `clap_builder` | 4.6.7 | `clap` | `clap_builder::builder::command::Command` | `cli_doc::CliError::from_clap`, `cli_doc::CommandSpec::from_clap`, `cli_doc::clap::parse_or_exit_with_spec`, `cli_doc::clap::parse_or_exit`, and 4 more |
| `clap_builder` | 4.6.7 | `clap` | `clap_builder::parser::matches::arg_matches::ArgMatches` | `cli_doc::clap::parse_or_exit_with_spec`, `cli_doc::clap::parse_or_exit`, `cli_doc::clap::try_parse_from`, `cli_doc::clap::try_parse_with_spec`, and 1 more |
| `log` | 0.4.35 | feature `log` | `log::LevelFilter` | `adapters::LogAdapter::new` |
| `log` | 0.4.35 | feature `log` | `log::Log` | `impl Log for adapters::LogAdapter` |
| `log` | 0.4.35 | feature `log` | `log::Metadata` | `adapters::LogAdapter::enabled` |
| `log` | 0.4.35 | feature `log` | `log::Record` | `adapters::LogAdapter::log` |
| `serde_core` | 1.0.229 | `serde` (the derive's traits live in `serde_core`) | `serde_core::de::Deserialize` | `impl Deserialize for diff::test_report::Case`, `impl Deserialize for diff::test_report::Status`, `impl Deserialize for diff::test_report::Suite`, `impl Deserialize for diff::test_report::TestRun`, and 23 more |
| `serde_core` | 1.0.229 | `serde` (the derive's traits live in `serde_core`) | `serde_core::de::Deserializer` | `diff::test_report::Case::deserialize`, `diff::test_report::Status::deserialize`, `diff::test_report::Suite::deserialize`, `diff::test_report::TestRun::deserialize`, and 23 more |
| `serde_core` | 1.0.229 | `serde` (the derive's traits live in `serde_core`) | `serde_core::ser::Error` | `impl Error for data::DataError` |
| `serde_core` | 1.0.229 | `serde` (the derive's traits live in `serde_core`) | `serde_core::ser::Serialize` | `data::from_serialize`, `data::json`, `data::print_json_to`, `data::print_json`, and 56 more |
| `serde_core` | 1.0.229 | `serde` (the derive's traits live in `serde_core`) | `serde_core::ser::Serializer` | `a11y::contrast::Deficiency::serialize`, `a11y::contrast::Finding::serialize`, `a11y::contrast::FindingKind::serialize`, `a11y::contrast::Severity::serialize`, and 45 more |
| `serde_json` | 1.0.154 | feature `data`, `test-report`, `testing` | `serde_json::error::Error` | `testing::SnapshotError` |
| `serde_json` | 1.0.154 | feature `data`, `test-report`, `testing` | `serde_json::error::Result` | `qa::lint::LintReport::from_json` |
| `serde_json` | 1.0.154 | feature `data`, `test-report`, `testing` | `serde_json::value::Value` | `data::Node::from`, `data::Node::to_json`, `deps::DepGraph::from_value`, `deps::audit::AdvisoryReport::from_cargo_audit_value`, and 17 more |
| `toml` | 1.1.6+spec-1.1.0 | feature `toml` | `toml::value::Value` | `table::rules::StyleRules::from_toml_value` |
| `tracing-core` | 0.1.36 | `tracing` / `tracing-subscriber` | `tracing_core::event::Event` | `adapters::EventLayer::on_event` |
| `tracing-core` | 0.1.36 | `tracing` / `tracing-subscriber` | `tracing_core::span::Attributes` | `adapters::EventLayer::on_new_span` |
| `tracing-core` | 0.1.36 | `tracing` / `tracing-subscriber` | `tracing_core::span::Id` | `adapters::EventLayer::on_close`, `adapters::EventLayer::on_new_span`, `adapters::EventLayer::on_record` |
| `tracing-core` | 0.1.36 | `tracing` / `tracing-subscriber` | `tracing_core::span::Record` | `adapters::EventLayer::on_record` |
| `tracing-core` | 0.1.36 | `tracing` / `tracing-subscriber` | `tracing_core::subscriber::Subscriber` | `impl Layer for adapters::EventLayer` |
| `tracing-subscriber` | 0.3.23 | feature `tracing` | `tracing_subscriber::layer::Layer` | `impl Layer for adapters::EventLayer` |
| `tracing-subscriber` | 0.3.23 | feature `tracing` | `tracing_subscriber::layer::context::Context` | `adapters::EventLayer::on_close`, `adapters::EventLayer::on_event`, `adapters::EventLayer::on_new_span`, `adapters::EventLayer::on_record` |
| `tracing-subscriber` | 0.3.23 | feature `tracing` | `tracing_subscriber::registry::LookupSpan` | `impl Layer for adapters::EventLayer` |

#### rs-rich-cli

No foreign type.

#### rs-rich-marketplace

| foreign crate | version | in the API | type | items |
|---|---|---|---|---|
| `http` | 1.5.0 | `ureq` | `http::response::Response` | `redirect::get_following` |
| `semver` | 1.0.28 | always | (the whole crate, re-exported) | `semver` |
| `semver` | 1.0.28 | always | `semver::Version` | `ListedVersion::version`, `Listing::version`, `Request::Exact` |
| `ureq` | 3.5.1 | feature `http` | `ureq::body::Body` | `redirect::get_following` |
| `ureq` | 3.5.1 | feature `http` | `ureq::error::Error` | `impl From for tls::TrustError`, `redirect::Deadline::remaining`, `redirect::get_following`, `tls::TrustError::bundled`, and 6 more |
| `ureq` | 3.5.1 | feature `http` | `ureq::tls::TlsConfig` | `tls::Trust::tls_config` |

#### rs-rich-art

| foreign crate | version | in the API | type | items |
|---|---|---|---|---|
| `image` | 0.25.10 | feature `avif`, `gif`, `iterm`, `kitty`, `picture`, `sixel` | (the whole crate, re-exported) | `image` |
| `image` | 0.25.10 | feature `avif`, `gif`, `iterm`, `kitty`, `picture`, `sixel` | `image::error::ImageError` | `ascii::AsciiArt::from_bytes`, `ascii::AsciiArt::from_path`, `block::BlockArt::from_path`, `gif::AnimatedArt::from_bytes`, and 6 more |
| `image` | 0.25.10 | feature `avif`, `gif`, `iterm`, `kitty`, `picture`, `sixel` | `image::images::buffer::RgbaImage` | `graphics::AnimationFrame::image`, `graphics::encode_png`, `graphics::fill_pixels`, `graphics::fit_to_cells`, and 5 more |
| `image` | 0.25.10 | feature `avif`, `gif`, `iterm`, `kitty`, `picture`, `sixel` | `image::images::dynimage::DynamicImage` | `ascii::AsciiArt::new`, `block::BlockArt::new`, `braille::BrailleArt::new`, `diff`, and 15 more |

#### rs-rich-micro

| foreign crate | version | in the API | type | items |
|---|---|---|---|---|
| `image` | 0.25.10 | rs-rich-art's `pub use image` | `image::images::buffer::RgbaImage` | `pipeline::Magnified`, `pipeline::Processed::still` |
| `image` | 0.25.10 | rs-rich-art's `pub use image` | `image::images::dynimage::DynamicImage` | `pipeline::Pipeline::process_image` |
| `serde_json` | 1.0.154 | always | `serde_json::value::Value` | `create::PackageSpec::manifest` |

#### rs-rich-record

| foreign crate | version | in the API | type | items |
|---|---|---|---|---|
| `fancy-regex` | 0.19.2 | always | `fancy_regex::Regex` | `Recording::masks`, `Tape::masks`, `record::Recording::masks`, `tape::Pattern::Regex`, and 2 more |
| `vt100` | 0.16.2 | always | `vt100::screen::Screen` | `screen::Snapshot::from_screen`, `terminal::Terminal::screen` |

#### rs-rich-interact

| foreign crate | version | in the API | type | items |
|---|---|---|---|---|
| `crossterm` | 0.29.0 | always | `crossterm::event::Event` | `event::from_crossterm_kitty`, `event::from_crossterm` |
| `serde_json` | 1.0.154 | feature `control`, `host` | `serde_json::value::Value` | `control::Reply`, `control::Request::args`, `control::Request::new`, `host::panes_of`, and 1 more |
| `termion` | 4.0.6 | feature `termion` | `termion::event::Event` | `event::from_termion` |
| `termwiz` | 0.23.3 | feature `termwiz` | `termwiz::input::InputEvent` | `event::from_termwiz_kitty`, `event::from_termwiz` |

#### rs-rich-intuituive

| foreign crate | version | in the API | type | items |
|---|---|---|---|---|
| `image` | 0.25.10 | rs-rich-art's `pub use image` | `image::images::dynimage::DynamicImage` | `sprite_sheet::SpriteSheet::from_image` |
| `tracing-core` | 0.1.36 | `tracing` / `tracing-subscriber` | `tracing_core::dispatcher::SetGlobalDefaultError` | `log::TracingFeed::install`, `log::install_tracing` |

#### rs-rich-embed

| foreign crate | version | in the API | type | items |
|---|---|---|---|---|
| `portable-pty` | 0.9.0 | always | `portable_pty::ExitStatus` | `ExitStatus::from`, `impl From for ExitStatus` |
| `serde_json` | 1.0.154 | feature `chrome` | `serde_json::value::Value` | `DevToolsEvent::params`, `chrome::DevTools::call`, `chrome::DevToolsEvent::params`, `chrome::key_events` |
| `vt100` | 0.16.2 | always | `vt100::screen::MouseProtocolEncoding` | `keys::mouse_bytes` |
| `vt100` | 0.16.2 | always | `vt100::screen::MouseProtocolMode` | `keys::mouse_bytes` |

#### noctiluca

| foreign crate | version | in the API | type | items |
|---|---|---|---|---|
| `image` | 0.25.10 | always | `image::images::buffer::RgbaImage` | `pixel_view::Screen::take`, `pixel_view::rgba` |
| `serde_json` | 1.0.154 | always | `serde_json::value::Value` | `control::ActionArgs::args`, `control::call`, `control::find_rows`, `control::handle`, and 6 more |

#### rs-rich-acp

| foreign crate | version | in the API | type | items |
|---|---|---|---|---|
| `serde_core` | 1.0.229 | `serde` (the derive's traits live in `serde_core`) | `serde_core::de::Deserialize` | `impl Deserialize for AgentCapabilities`, `impl Deserialize for AudioContent`, `impl Deserialize for AuthMethodId`, `impl Deserialize for AuthMethod`, and 71 more |
| `serde_core` | 1.0.229 | `serde` (the derive's traits live in `serde_core`) | `serde_core::de::DeserializeOwned` | `Connection::request`, `impl<R: serde_core::de::DeserializeOwned> Pending<` |
| `serde_core` | 1.0.229 | `serde` (the derive's traits live in `serde_core`) | `serde_core::de::Deserializer` | `AgentCapabilities::deserialize`, `AudioContent::deserialize`, `AuthMethod::deserialize`, `AuthMethodId::deserialize`, and 71 more |
| `serde_core` | 1.0.229 | `serde` (the derive's traits live in `serde_core`) | `serde_core::ser::Serialize` | `Connection::notify`, `Connection::request`, `impl Serialize for AgentCapabilities`, `impl Serialize for AudioContent`, and 73 more |
| `serde_core` | 1.0.229 | `serde` (the derive's traits live in `serde_core`) | `serde_core::ser::Serializer` | `AgentCapabilities::serialize`, `AudioContent::serialize`, `AuthMethod::serialize`, `AuthMethodId::serialize`, and 71 more |
| `serde_json` | 1.0.154 | always | `serde_json::error::Error` | `Error::Json`, `Error::from`, `impl From for Error` |
| `serde_json` | 1.0.154 | always | `serde_json::value::Value` | `AgentCapabilities::session_capabilities`, `AudioContent::annotations`, `AvailableCommand::input`, `Client::ext_method`, and 17 more |

#### cornacai

No foreign type.
