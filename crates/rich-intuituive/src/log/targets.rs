//! [`Log::targets_view`]: the targets a log has seen, each with what it
//! shows of them, set from the keyboard or the mouse (tui-logger's target
//! selector).

use rich_interact::{Key, KeyCode, MouseKind};

use super::{level_look, Log, Severity, TargetLevel};
use crate::node::{Axis, Node};
use crate::screen::Rect;
use crate::widget::{Canvas, DrawCx, EventCx, MeasureCx, MouseExt, Used, Widget, WidgetEvent};

/// The settings Left and Right step through, from showing nothing to
/// showing everything.
const STEPS: [TargetLevel; 6] = [
    TargetLevel::Off,
    TargetLevel::From(Severity::Error),
    TargetLevel::From(Severity::Warn),
    TargetLevel::From(Severity::Info),
    TargetLevel::From(Severity::Debug),
    TargetLevel::From(Severity::Trace),
];

/// Where `level` (or, for no level, everything) is in [`STEPS`].
fn step_of(level: Option<Severity>) -> usize {
    match level {
        None | Some(Severity::Trace) => 5,
        Some(Severity::Debug) => 4,
        Some(Severity::Info) => 3,
        Some(Severity::Warn) => 2,
        Some(Severity::Error) | Some(Severity::Fatal) => 1,
    }
}

/// Where a target's setting is in [`STEPS`], the log's level standing in
/// for [`TargetLevel::Inherit`].
fn effective(level: TargetLevel, log_level: Option<Severity>) -> usize {
    match level {
        TargetLevel::Off => 0,
        TargetLevel::From(level) => step_of(Some(level)),
        TargetLevel::Inherit => step_of(log_level),
    }
}

pub(super) fn view(log: Log) -> Node {
    Node::from_widget(
        Box::new(TargetsView {
            log,
            selected: 0,
            first: 0,
            height: 0,
        }),
        "log-targets",
    )
    .focusable()
}

struct TargetsView {
    log: Log,
    selected: usize,
    first: usize,
    height: usize,
}

impl TargetsView {
    fn count(&self) -> usize {
        self.log.data.with_untracked(|data| data.targets.len())
    }

    fn selected_target(&self) -> Option<String> {
        self.log.data.with_untracked(|data| {
            data.targets
                .get(self.selected)
                .map(|(name, _)| name.clone())
        })
    }

    fn select(&mut self, cx: &mut EventCx, to: usize) -> Used {
        let to = to.min(self.count().saturating_sub(1));
        if to != self.selected {
            self.selected = to;
            cx.redraw();
        }
        Used::Yes
    }

    /// Set the chosen target's setting.
    fn set(&self, change: impl FnOnce(TargetLevel, Option<Severity>) -> TargetLevel) -> Used {
        let Some(target) = self.selected_target() else {
            return Used::No;
        };
        let (own, log_level) = self.log.data.with_untracked(|data| {
            let own = data
                .filters
                .targets
                .iter()
                .find(|(name, _)| *name == target)
                .map_or(TargetLevel::Inherit, |(_, level)| *level);
            (own, data.filters.level)
        });
        let to = change(own, log_level);
        if to != own {
            self.log.set_target_level(&target, to);
        }
        Used::Yes
    }

    fn key(&mut self, cx: &mut EventCx, key: &Key) -> Used {
        if key.modifiers.ctrl || key.modifiers.alt || self.count() == 0 {
            return Used::No;
        }
        let page = self.height.saturating_sub(1).max(1);
        match key.code {
            KeyCode::Up => self.select(cx, self.selected.saturating_sub(1)),
            KeyCode::Down => self.select(cx, self.selected + 1),
            KeyCode::PageUp => self.select(cx, self.selected.saturating_sub(page)),
            KeyCode::PageDown => self.select(cx, self.selected + page),
            KeyCode::Home => self.select(cx, 0),
            KeyCode::End => self.select(cx, usize::MAX),
            KeyCode::Left => {
                self.set(|own, log_level| STEPS[effective(own, log_level).saturating_sub(1)])
            }
            KeyCode::Right => self
                .set(|own, log_level| STEPS[(effective(own, log_level) + 1).min(STEPS.len() - 1)]),
            KeyCode::Char(' ') => self.set(|own, log_level| {
                if effective(own, log_level) == 0 {
                    TargetLevel::Inherit
                } else {
                    TargetLevel::Off
                }
            }),
            KeyCode::Backspace | KeyCode::Delete => self.set(|_, _| TargetLevel::Inherit),
            _ => Used::No,
        }
    }
}

impl Widget for TargetsView {
    fn name(&self) -> &'static str {
        "log-targets"
    }

    fn role(&self) -> crate::a11y::Role {
        crate::a11y::Role::List
    }

    fn focusable(&self) -> bool {
        true
    }

    fn describe(&self) -> Option<String> {
        Some(format!("{} targets", self.count()))
    }

    /// The chosen target and what the log shows of it.
    fn access_text(&self) -> Option<String> {
        let target = self.selected_target()?;
        let shows = match self.log.target_level(&target) {
            TargetLevel::Off => "off".to_string(),
            TargetLevel::From(level) => format!("{} and above", level.as_str()),
            TargetLevel::Inherit => match self.log.level() {
                Some(level) => format!("{} and above, as the log", level.as_str()),
                None => "everything, as the log".to_string(),
            },
        };
        Some(format!("{target}: {shows}"))
    }

    fn cursor(&self) -> Option<Rect> {
        let row = self.selected.checked_sub(self.first)?;
        (row < self.height && self.selected < self.count())
            .then(|| Rect::new(0, row as u16, u16::MAX, 1))
    }

    fn access_state(&self) -> crate::a11y::AccessState {
        crate::a11y::AccessState::item(self.selected, self.count())
    }

    fn measure(&mut self, _cx: &MeasureCx, axis: Axis, width: u16, _height: u16) -> u16 {
        match axis {
            Axis::Vertical => {
                let count = self.log.data.with(|data| data.targets.len());
                count.clamp(1, u16::MAX as usize) as u16
            }
            Axis::Horizontal => {
                let widest = self.log.data.with(|data| {
                    data.targets
                        .iter()
                        .map(|(name, _)| rich::cells::cell_len(name))
                        .max()
                        .unwrap_or(0)
                });
                (widest + 6).min(width as usize) as u16
            }
        }
    }

    fn draw(&mut self, cx: &mut DrawCx, canvas: &mut Canvas) {
        let height = canvas.height() as usize;
        self.height = height;
        let (targets, settings, log_level) = self.log.data.with(|data| {
            let settings: Vec<TargetLevel> = data
                .targets
                .iter()
                .map(|(name, _)| {
                    data.filters
                        .targets
                        .iter()
                        .find(|(own, _)| own == name)
                        .map_or(TargetLevel::Inherit, |(_, level)| *level)
                })
                .collect();
            (data.targets.clone(), settings, data.filters.level)
        });
        if targets.is_empty() {
            let dim = cx.style("log.target", "dim");
            canvas.print(0, 0, "no targets yet", Some(&dim));
            return;
        }
        self.selected = self.selected.min(targets.len() - 1);
        if self.selected < self.first {
            self.first = self.selected;
        } else if height > 0 && self.selected >= self.first + height {
            self.first = self.selected + 1 - height;
        }
        self.first = self.first.min(targets.len().saturating_sub(height.max(1)));
        let off = cx.style("log.target.off", "dim strike");
        let inherited = cx.style("log.target", "dim");
        let highlight = if cx.focused() {
            cx.style("selected", "reverse")
        } else {
            cx.style("log.targets.selected", "underline")
        };
        let width = canvas.width();
        for (row, i) in (self.first..targets.len()).take(height).enumerate() {
            let (name, count) = &targets[i];
            let step = effective(settings[i], log_level);
            let (label, style) = match STEPS[step] {
                TargetLevel::From(level) => {
                    let (label, theme, fallback) = level_look(level);
                    (label, cx.style(theme, fallback))
                }
                _ => ("OFF  ", off.clone()),
            };
            let y = row as u16;
            let label_style = if settings[i] == TargetLevel::Inherit && step != 0 {
                inherited.clone()
            } else {
                style
            };
            canvas.print(0, y, label, Some(&label_style));
            let name_style = (step == 0).then_some(&off);
            canvas.print(6, y, name, name_style);
            let count = count.to_string();
            let at = width.saturating_sub(count.len() as u16);
            let name_end = 6 + rich::cells::cell_len(name) as u16;
            if at > name_end {
                canvas.print(at, y, &count, Some(&inherited));
            }
            if i == self.selected {
                canvas.restyle(0, y, width, 1, &highlight);
            }
        }
    }

    fn event(&mut self, cx: &mut EventCx, event: &WidgetEvent) -> Used {
        match event {
            WidgetEvent::Key(key) => self.key(cx, key),
            WidgetEvent::Mouse(mouse) => match mouse.kind {
                MouseKind::Down(_) => {
                    let at = self.first + mouse.row as usize;
                    if at < self.count() {
                        self.select(cx, at)
                    } else {
                        Used::No
                    }
                }
                _ => match mouse.wheel() {
                    0 => Used::No,
                    -1 => self.select(cx, self.selected.saturating_sub(1)),
                    _ => self.select(cx, self.selected + 1),
                },
            },
            _ => Used::No,
        }
    }
}
