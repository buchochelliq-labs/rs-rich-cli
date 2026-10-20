//! The effect presets of `intuituive::fx`, a gallery: each panel plays one,
//! over and over, after a splash screen.
//!
//!     cargo run -p rs-rich-intuituive --example fx
//!
//! Enter skips the splash · `l` toggles the skeleton's loading · `t` shows
//! and hides the drawer (a transition) · `g` glitches the title · q quits.
//! Run with `INTUITUIVE_ACCESSIBLE=1` to see every effect at its end, with
//! no motion.

use std::time::Duration;

use intuituive::fx::{self, Fx};
use intuituive::prelude::*;
use intuituive::Direction;

fn ms(n: u64) -> Duration {
    Duration::from_millis(n)
}

/// Text to show an effect on.
const LOREM: &str = "[bold cyan]Effects[/] rework\nwhat a node drew,\n[magenta]cell by cell[/].";

/// In, held, out, held, for ever.
fn cycle(show: Fx, hide: Fx) -> Fx {
    fx::sequence([show.hold(ms(900)), hide.hold(ms(400))]).forever()
}

/// One panel of the gallery.
fn tile(title: &str, body: Node, fx: Fx) -> Node {
    fx::play(body, fx).panel(title)
}

/// The gallery, without the splash.
pub fn gallery() -> Node {
    let loading = signal(true);
    let drawer = signal(false);
    let glitches = signal(0u32);
    let tiles = [
        tile(
            "fade_in / fade_out",
            label(LOREM),
            cycle(fx::fade_in(ms(700)), fx::fade_out(ms(700))),
        ),
        tile(
            "coalesce / dissolve",
            label(LOREM),
            cycle(fx::coalesce(ms(900)), fx::dissolve(ms(900))),
        ),
        tile(
            "sweep",
            label(LOREM),
            cycle(
                fx::sweep(Direction::Right, ms(700)),
                fx::sweep(Direction::Down, ms(500)).reversed(),
            ),
        ),
        tile(
            "slide_in",
            label(LOREM),
            cycle(
                fx::slide_in(Direction::Left, ms(500)),
                fx::fade_out(ms(300)),
            ),
        ),
        tile(
            "hsl_shift",
            label(
                "[#e04040]red[/] [#40c040]green[/] [#4060e0]blue[/]\n[on #303080] a background [/]",
            ),
            fx::hsl_shift(360.0, 0.0, 0.0, ms(3000))
                .easing(intuituive::Easing::Linear)
                .forever(),
        ),
        tile(
            "glitch",
            label(LOREM),
            fx::sequence([fx::wait(ms(1500)), fx::glitch(ms(500))]).forever(),
        ),
        tile("rain", label(LOREM), fx::rain()),
        tile(
            "shimmer",
            label("[grey50]Loading your\nworkspace…"),
            fx::shimmer(ms(1600)),
        ),
        fx::play_while(
            label("Ada Lovelace   1815\nAlan Turing    1912\nGrace Hopper   1906"),
            fx::skeleton(ms(1200)),
            move || loading.get(),
        )
        .panel("skeleton (l)"),
    ];
    let title = fx::play_when(
        label("[b]intuiTUIve fx[/] · l loading · t drawer · g glitch · q quits"),
        fx::glitch(ms(400)),
        move || glitches.get() % 2 == 1,
    )
    .fixed(1);
    let drawer_panel = fx::transition(
        label("A drawer, shown and hidden\nwith a slide and a fade.").panel("transition (t)"),
        fx::parallel([fx::slide_in(Direction::Down, ms(300)), fx::fade_in(ms(300))]),
        move || drawer.get(),
    )
    .fixed(4);
    column([
        title,
        grid([Size::Flex(1), Size::Flex(1), Size::Flex(1)], tiles),
        drawer_panel,
    ])
    .on_key("l", move |_| loading.update(|l| *l = !*l))
    .on_key("t", move |_| drawer.update(|d| *d = !*d))
    .on_key("g", move |cx| {
        // Each press plays it once: the condition turns true, then false.
        glitches.update(|g| *g += 1);
        cx.after(ms(450), move |_| glitches.update(|g| *g += 1));
    })
    .on_key("q", |cx| cx.quit())
}

/// The app: a splash, then the gallery.
pub fn app() -> App {
    App::new(|| {
        let cover = label("[bold magenta]intuiTUIve[/]\n[dim]effect presets")
            .text_align(TextAlign::Center)
            .panel("");
        fx::splash(cover, ms(800), gallery())
    })
}

fn main() -> std::io::Result<()> {
    app().run()
}
