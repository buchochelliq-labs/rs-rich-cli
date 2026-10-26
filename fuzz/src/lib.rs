//! Helpers shared by the fuzz targets in `fuzz_targets/`.
//!
//! Each target must not panic on any input. Rendering targets cap their work
//! (input length and console width), so a legitimately large input is not
//! reported as a timeout: the bounds are here, in one place.

use rich::color::ColorSystem;
use rich::protocol::Renderable;
use rich::Console;

/// The longest input a rendering target reads, in bytes. Larger inputs are
/// cut, not skipped, so the fuzzer still learns from their start.
pub const MAX_RENDER_INPUT: usize = 4 * 1024;

/// The longest input a parse-only target reads, in bytes.
pub const MAX_PARSE_INPUT: usize = 64 * 1024;

/// The widest console a rendering target draws on.
pub const MAX_WIDTH: usize = 160;

/// `data` cut to `max` bytes and read as UTF-8, with invalid bytes replaced.
pub fn text(data: &[u8], max: usize) -> String {
    String::from_utf8_lossy(&data[..data.len().min(max)]).into_owned()
}

/// Split off the first byte as a parameter (a width, a size, a mode), and
/// return it with the rest. Empty input gives 0 and nothing.
pub fn take_byte(data: &[u8]) -> (u8, &[u8]) {
    match data.split_first() {
        Some((first, rest)) => (*first, rest),
        None => (0, data),
    }
}

/// A console `width` columns wide (1 to [`MAX_WIDTH`]), with colour,
/// writing nowhere but the returned strings.
pub fn console(width: u8) -> Console {
    let width = usize::from(width) % MAX_WIDTH + 1;
    Console::builder()
        .force_terminal(true)
        .color_system(Some(ColorSystem::Truecolor))
        .no_color(false)
        .width(width)
        .build()
}

/// Render `renderable` on a console `width` columns wide.
pub fn render(width: u8, renderable: &dyn Renderable) -> String {
    console(width).render_to_string(renderable)
}

/// rs-rich-marketplace's test keys (`crates/rich-marketplace/tests/fixtures/keys`):
/// trusted only by its tests and these targets, never by a build.
pub mod test_keys {
    use std::io::Cursor;
    use std::sync::OnceLock;

    use rich_marketplace::{KeySet, TrustedKey};

    /// One test pair: the signing half and the key the client trusts.
    pub struct Pair {
        secret: minisign::SecretKey,
        pub trusted: TrustedKey,
    }

    impl Pair {
        fn load(public: &str, secret: &str) -> Pair {
            let secret = minisign::SecretKeyBox::from_string(secret)
                .and_then(minisign::SecretKey::from_unencrypted_box)
                .expect("an unencrypted minisign test key");
            let trusted = TrustedKey::from_minisign_pub(public).expect("a minisign public key");
            Pair { secret, trusted }
        }

        /// A `.minisig` over `data` with this trusted comment.
        pub fn sign(&self, data: &[u8], comment: &str) -> String {
            minisign::sign(None, &self.secret, Cursor::new(data), Some(comment), None)
                .expect("signing")
                .to_string()
        }
    }

    /// Generation 1's main key.
    pub fn main() -> &'static Pair {
        static PAIR: OnceLock<Pair> = OnceLock::new();
        PAIR.get_or_init(|| {
            Pair::load(
                include_str!("../../crates/rich-marketplace/tests/fixtures/keys/gen1-main.pub"),
                include_str!("../../crates/rich-marketplace/tests/fixtures/keys/gen1-main.key"),
            )
        })
    }

    /// Generation 1's backup key.
    pub fn backup() -> &'static Pair {
        static PAIR: OnceLock<Pair> = OnceLock::new();
        PAIR.get_or_init(|| {
            Pair::load(
                include_str!("../../crates/rich-marketplace/tests/fixtures/keys/gen1-backup.pub"),
                include_str!("../../crates/rich-marketplace/tests/fixtures/keys/gen1-backup.key"),
            )
        })
    }

    /// Generation 1, as the marketplace's own tests build it.
    pub fn generation_1() -> KeySet {
        KeySet::new(1, main().trusted.clone(), backup().trusted.clone()).expect("generation 1")
    }
}

/// A fixed "now" for index expiry, so a run does not depend on the clock:
/// late October 2026, when the targets were added.
pub fn now() -> std::time::SystemTime {
    std::time::UNIX_EPOCH + std::time::Duration::from_secs(1_793_000_000)
}
