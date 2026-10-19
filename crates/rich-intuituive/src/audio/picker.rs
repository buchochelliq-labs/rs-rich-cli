//! Choosing an input inside an app: a list over [`devices`].

use std::convert::Infallible;

use super::capture::{devices, Device};
use crate::app::Ctx;
use crate::node::Node;
use crate::reactive::{signal, watch};
use crate::task::{resource, Load};

/// A list of the inputs, as Bubble Tea's list is in audio TUIs: each row
/// the device's name with `(default)` after the default, which is selected
/// first. Enter calls `on_pick` with the selected [`Device`] (open it with
/// [`AudioOptions::device`](super::AudioOptions::device)`(&device.id)`), `r`
/// lists them again. The devices are listed on a worker thread (ALSA's
/// probing can take a moment): `looking for inputs…` until then, `no audio
/// inputs` when there are none.
///
/// ```no_run
/// use intuituive::audio;
/// use intuituive::prelude::*;
///
/// App::new(|| {
///     let chosen = signal(String::new());
///     column([
///         audio::picker(move |device, _| chosen.set(device.id.clone())).panel("Input"),
///         text!("[dim]using {chosen}"),
///     ])
///     .on_key("q", |cx| cx.quit())
/// })
/// .run()
/// # .unwrap();
/// ```
pub fn picker(on_pick: impl FnMut(&Device, &mut Ctx) + 'static) -> Node {
    picker_of(devices, on_pick)
}

/// [`picker`] over the devices `list` finds.
pub(crate) fn picker_of(
    list: impl Fn() -> Vec<Device> + Send + Sync + 'static,
    mut on_pick: impl FnMut(&Device, &mut Ctx) + 'static,
) -> Node {
    let found = resource(move || Ok::<_, Infallible>(list()));
    let selected = signal(0usize);
    // Select the default once they are listed.
    watch(
        move || found.with(|load| matches!(load, Load::Ready(_))),
        move |ready, _| {
            if ready {
                let default = found.with(|load| match load {
                    Load::Ready(devices) => devices.iter().position(|d| d.default),
                    _ => None,
                });
                selected.set(default.unwrap_or(0));
            }
        },
    );
    let items = move || {
        found.with(|load| match load {
            Load::Ready(devices) if devices.is_empty() => vec!["[dim]no audio inputs".into()],
            Load::Ready(devices) => devices
                .iter()
                .map(|d| {
                    let default = if d.default { " [dim](default)" } else { "" };
                    format!("{}{default}", rich::markup::escape(&d.name))
                })
                .collect(),
            Load::Failed(why) => vec![format!("[bad]{}", rich::markup::escape(why))],
            Load::Loading => vec!["[dim]looking for inputs…".into()],
        })
    };
    crate::list::list(items, selected)
        .bind("enter", "use this input", move |cx| {
            let device = found.with(|load| match load {
                Load::Ready(devices) => devices.get(selected.get_untracked()).cloned(),
                _ => None,
            });
            if let Some(device) = device {
                on_pick(&device, cx);
            }
        })
        .bind("r", "list the inputs again", move |_| found.reload())
        .name("audio picker")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::audio::Host;

    fn device(id: &str, name: &str, default: bool) -> Device {
        Device {
            id: id.into(),
            name: name.into(),
            default,
            host: Host::Default,
        }
    }

    #[test]
    fn enter_picks_the_selected_device_starting_at_the_default() {
        let app = crate::App::new(|| {
            let picked = signal(String::new());
            let list = || {
                vec![
                    device("hw:0", "Built-in [Mic]", false),
                    device("default", "Default", true),
                    device("null", "Discard", false),
                ]
            };
            crate::column([
                picker_of(list, move |d, _| picked.set(d.id.clone()))
                    .autofocus()
                    .flex(1),
                crate::text(move || format!("picked {}", picked.get())).fixed(1),
            ])
        });
        let mut driver = app.driver(30, 4);
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
        while !driver.screen().plain()[0].contains("Built-in") {
            assert!(std::time::Instant::now() < deadline, "never listed");
            std::thread::sleep(std::time::Duration::from_millis(1));
            driver.settle();
        }
        let screen = driver.screen().plain();
        assert_eq!(screen[0].trim_end(), "Built-in [Mic]");
        assert_eq!(screen[1].trim_end(), "Default (default)");
        driver.press("enter");
        assert_eq!(driver.screen().plain()[3].trim_end(), "picked default");
        driver.press("down");
        driver.press("enter");
        assert_eq!(driver.screen().plain()[3].trim_end(), "picked null");
    }

    #[test]
    fn no_devices_says_so() {
        let app = crate::App::new(|| picker_of(Vec::new, |_, _| {}));
        let mut driver = app.driver(20, 1);
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
        while !driver.screen().plain()[0].contains("no audio") {
            assert!(std::time::Instant::now() < deadline, "never listed");
            std::thread::sleep(std::time::Duration::from_millis(1));
            driver.settle();
        }
    }
}
