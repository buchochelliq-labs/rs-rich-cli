//! A dial: a value in a range drawn as an arc, the reading in its middle
//! ([`dial`]). Speed tests, a car's gauges, a level against its limits.

use std::ops::RangeInclusive;

use rich::Style;
use rich::color::Color;

use crate::a11y::Role;
use crate::controls::look;
use crate::node::{Axis, Node};
use crate::pixels::PixelMode;
use crate::widget::{Canvas, DrawCx, MeasureCx, Widget, widget};

/// The smallest box a dial draws an arc in; in a smaller one it is a bar
/// with the reading.
const MIN_ARC: (u16, u16) = (10, 5);

/// The arc's thickness, as a share of its radius: thinner in Braille,
/// whose dots are finer.
fn thickness(mode: PixelMode) -> f64 {
    match mode {
        PixelMode::Braille => 0.17,
        _ => 0.22,
    }
}

/// One part of a dial's range drawn in its own style: up to `upto`, which
/// it holds when `inclusive`.
#[derive(Clone, Debug, PartialEq)]
struct Zone {
    upto: f64,
    inclusive: bool,
    style: String,
}

/// What [`dial`] makes.
pub(crate) struct Dial {
    value: Box<dyn Fn() -> f64>,
    min: f64,
    max: f64,
    sweep: f64,
    readout: Option<Box<dyn Fn() -> String>>,
    unit: Option<String>,
    caption: Option<String>,
    zones: Vec<Zone>,
    mode: PixelMode,
    ends: bool,
    /// What the last draw said: the reading and its unit.
    shown: Option<String>,
}

/// A dial over `range`, showing what `value` returns (a closure that may
/// read signals: the dial draws again when they change): an arc of
/// [`sweep`](Node::sweep) degrees (270 by default, open at the bottom)
/// filled up to the value, the reading in its middle, in big
/// [`digits`](crate::digits) when there is room, its
/// [`unit`](Node::unit) under it, and the range's ends under the arc's
/// ends. The arc is drawn in Braille dots (or half blocks:
/// [`arc_mode`](Node::arc_mode)), so it is round at any size from 10 by 5
/// cells; in a smaller box, or in [text mode](crate::App::accessible), it
/// is one row: a bar and the reading.
///
/// The filled arc and the reading take the style of the zone the value is
/// in ([`bands`](Node::bands), as a `<meter>`'s low, high and optimum, or
/// [`zones`](Node::zones)), else the theme's `dial.fill` (else `accent`);
/// the rest of the arc is `dial.track` (else `muted`), the range's ends
/// and the unit `dial.label` (else `muted`), the
/// [`caption`](Node::caption) `dial.caption` (else `bold`).
///
/// Its role is [`Meter`](crate::a11y::Role::Meter), its value the reading
/// with its unit: `Download, meter: 287.4 Mbps`.
///
/// ```
/// use intuituive::prelude::*;
/// use intuituive::dial;
///
/// let app = App::new(|| {
///     let speed = signal(287.4);
///     dial(move || speed.get(), 0.0..=1000.0)
///         .unit("Mbps")
///         .caption("Download")
/// });
/// let mut driver = app.driver(30, 12);
/// driver.settle();
/// let shown = driver.screen().plain().join("\n");
/// assert!(shown.contains("Mbps") && shown.contains("Download"));
/// // The arc, in Braille dots.
/// assert!(shown.chars().any(|c| ('\u{2801}'..='\u{28ff}').contains(&c)));
/// assert_eq!(
///     driver.accessibility()[0].describe(),
///     "Download, meter: 287 Mbps"
/// );
/// ```
pub fn dial(value: impl Fn() -> f64 + 'static, range: RangeInclusive<f64>) -> Node {
    let (min, max) = (*range.start(), *range.end());
    let (min, max) = if min <= max { (min, max) } else { (max, min) };
    widget(Dial {
        value: Box::new(value),
        min,
        max,
        sweep: 270.0,
        readout: None,
        unit: None,
        caption: None,
        zones: Vec::new(),
        mode: PixelMode::Braille,
        ends: true,
        shown: None,
    })
}

/// Which part of the arc a pixel is on.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Dot {
    Off,
    Track,
    Fill,
}

/// Where the arc went, in cells: its middle row, the inner radius's
/// half width in cells there, and the columns and row of its two ends.
struct Placed {
    middle: u16,
    inner: (u16, u16),
    ends: ((u16, u16), (u16, u16)),
}

impl Dial {
    /// Decimals for a number of this range: none for a range of 100 or
    /// more, one for 10 or more, else two.
    fn decimals(&self) -> usize {
        match self.max - self.min {
            span if span >= 100.0 => 0,
            span if span >= 10.0 => 1,
            _ => 2,
        }
    }

    fn number(&self, value: f64) -> String {
        format!("{value:.0$}", self.decimals())
    }

    /// Where `value` is along the range, from 0 to 1.
    fn fraction(&self, value: f64) -> f64 {
        let span = self.max - self.min;
        if span <= 0.0 || !value.is_finite() {
            return 0.0;
        }
        ((value - self.min) / span).clamp(0.0, 1.0)
    }

    /// The zone's style for `value`, or the fill's.
    fn fill_style(&self, cx: &DrawCx, value: f64) -> Style {
        let zone = self
            .zones
            .iter()
            .find(|z| value < z.upto || (z.inclusive && value == z.upto))
            .or(self.zones.last());
        match zone {
            Some(zone) => look(cx, &[zone.style.as_str()], &zone.style),
            None => look(cx, &["dial.fill", "accent"], "bold"),
        }
    }

    /// How far below its centre the arc reaches, as a share of its radius.
    fn below(&self) -> f64 {
        if self.sweep >= 360.0 {
            1.0
        } else {
            (-(self.sweep / 2.0).to_radians().cos()).max(0.0)
        }
    }

    /// Draw the arc in `width` x `rows` cells, filled to `fraction`.
    fn draw_arc(
        &self,
        canvas: &mut Canvas,
        (width, rows): (u16, u16),
        fraction: f64,
        (fill, track): (&Style, &Style),
    ) -> Placed {
        let (cw, ch) = match self.mode {
            PixelMode::Braille => (2u16, 4u16),
            _ => (1, 2),
        };
        let (pw, ph) = ((width * cw) as f64, (rows * ch) as f64);
        let below = self.below();
        let share = thickness(self.mode);
        // Pixels are square: a cell is twice as tall as it is wide.
        let radius = ((pw - 1.0) / (2.0 + share))
            .min((ph - 1.0) / (1.0 + below + share))
            .max(1.0);
        let thick = (radius * share).max(1.2);
        let (cx, cy) = (pw / 2.0, radius + thick / 2.0);
        let start = 90.0 + self.sweep.min(360.0) / 2.0;
        let dot = |x: u16, y: u16| -> Dot {
            let (dx, dy) = (x as f64 + 0.5 - cx, cy - (y as f64 + 0.5));
            if ((dx * dx + dy * dy).sqrt() - radius).abs() > thick / 2.0 {
                return Dot::Off;
            }
            let along = (start - dy.atan2(dx).to_degrees()).rem_euclid(360.0);
            if along > self.sweep {
                Dot::Off
            } else if fraction > 0.0 && along <= fraction * self.sweep {
                Dot::Fill
            } else {
                Dot::Track
            }
        };
        let track_color = track
            .color()
            .cloned()
            .unwrap_or_else(|| Color::parse("grey35").expect("a colour"));
        for row in 0..rows {
            for col in 0..width {
                let (x0, y0) = (col * cw, row * ch);
                if self.mode == PixelMode::Braille {
                    // In the order of the Braille pattern's bits.
                    const BITS: [(u16, u16); 8] = [
                        (0, 0),
                        (0, 1),
                        (0, 2),
                        (1, 0),
                        (1, 1),
                        (1, 2),
                        (0, 3),
                        (1, 3),
                    ];
                    let (mut mask, mut fills, mut tracks) = (0u32, 0, 0);
                    for (bit, (dx, dy)) in BITS.iter().enumerate() {
                        match dot(x0 + dx, y0 + dy) {
                            Dot::Off => continue,
                            Dot::Fill => fills += 1,
                            Dot::Track => tracks += 1,
                        }
                        mask |= 1 << bit;
                    }
                    if mask == 0 {
                        continue;
                    }
                    let glyph = char::from_u32(0x2800 + mask).unwrap_or(' ');
                    let style = if fills > 0 && fills >= tracks {
                        fill
                    } else {
                        track
                    };
                    canvas.set(col, row, glyph, Some(style));
                } else {
                    let (top, bottom) = (dot(x0, y0), dot(x0, y0 + 1));
                    let pick = |d: Dot| match d {
                        Dot::Fill => Some(fill),
                        Dot::Track => Some(track),
                        Dot::Off => None,
                    };
                    match (pick(top), pick(bottom)) {
                        (None, None) => {}
                        (Some(a), None) => canvas.set(col, row, '▀', Some(a)),
                        (None, Some(b)) => canvas.set(col, row, '▄', Some(b)),
                        (Some(a), Some(_)) if top == bottom => canvas.set(col, row, '█', Some(a)),
                        (Some(a), Some(b)) => {
                            let under = if std::ptr::eq(b, track) {
                                track_color.clone()
                            } else {
                                b.color().cloned().unwrap_or_else(|| track_color.clone())
                            };
                            let style = a.combine(&Style::new().with_bgcolor(under));
                            canvas.set(col, row, '▀', Some(&style));
                        }
                    }
                }
            }
        }
        let inner = radius - thick / 2.0;
        let cell = |x: f64, y: f64| {
            (
                (x / cw as f64).floor().clamp(0.0, (width - 1) as f64) as u16,
                (y / ch as f64).floor().clamp(0.0, (rows - 1) as f64) as u16,
            )
        };
        let end = |degrees: f64| {
            let r = degrees.to_radians();
            cell(cx + radius * r.cos(), cy - radius * r.sin() + thick / 2.0)
        };
        Placed {
            middle: (cy / ch as f64).floor() as u16,
            inner: (
                ((inner * 2.0 / cw as f64).floor() as u16).saturating_sub(1),
                (inner / ch as f64).floor() as u16,
            ),
            ends: (end(start), end(start - self.sweep.min(360.0))),
        }
    }

    /// One row: a bar filled to the value in eighths, and the reading.
    fn draw_compact(
        &self,
        canvas: &mut Canvas,
        (width, height): (u16, u16),
        fraction: f64,
        reading: &str,
        styles: (&Style, &Style, &Style),
    ) {
        let (fill, track, caption_style) = styles;
        let y = height.saturating_sub(1) / 2;
        let text_mode = crate::a11y::text_mode();
        let reading_w = rich::cells::cell_len(reading) as u16;
        let mut x = 0;
        if let Some(caption) = &self.caption {
            let caption_w = rich::cells::cell_len(caption) as u16;
            if caption_w + reading_w + 5 <= width {
                canvas.print(0, y, caption, Some(caption_style));
                x = caption_w + 1;
            }
        }
        let bar = width.saturating_sub(x + reading_w + 1);
        if bar < 3 {
            canvas.print(x, y, reading, Some(fill));
            return;
        }
        let eighths = (fraction * bar as f64 * 8.0).round() as u16;
        let (full, part) = (eighths / 8, eighths % 8);
        let (on, off) = if text_mode {
            ("#", "-")
        } else {
            ("█", "░")
        };
        canvas.print(x, y, &on.repeat(full as usize), Some(fill));
        let mut at = x + full;
        if part > 0 && !text_mode {
            let glyph = ['▏', '▎', '▍', '▌', '▋', '▊', '▉'][part as usize - 1];
            canvas.set(at, y, glyph, Some(fill));
            at += 1;
        }
        let rest = (x + bar).saturating_sub(at);
        canvas.print(at, y, &off.repeat(rest as usize), Some(track));
        canvas.print(x + bar + 1, y, reading, Some(fill));
    }
}

/// The columns `text` takes in big digits, if every character has a glyph.
fn digits_width(text: &str) -> Option<u16> {
    text.chars()
        .map(|c| crate::digits::glyph(c).map(|g| rich::cells::cell_len(g[0]) as u16))
        .sum()
}

impl Widget for Dial {
    fn name(&self) -> &'static str {
        "dial"
    }

    fn describe(&self) -> Option<String> {
        self.shown.clone()
    }

    fn role(&self) -> Role {
        Role::Meter
    }

    fn access_value(&self) -> Option<String> {
        self.shown.clone()
    }

    fn measure(&mut self, _cx: &MeasureCx, axis: Axis, width: u16, _height: u16) -> u16 {
        match axis {
            Axis::Horizontal => 24,
            Axis::Vertical => {
                let (cw, ch) = match self.mode {
                    PixelMode::Braille => (2.0, 4.0),
                    _ => (1.0, 2.0),
                };
                let share = thickness(self.mode);
                let radius = (width as f64 * cw - 1.0) / (2.0 + share);
                let rows = ((radius * (1.0 + self.below() + share) + 1.0) / ch).ceil() as u16;
                rows + u16::from(self.ends) + u16::from(self.caption.is_some())
            }
        }
    }

    fn draw(&mut self, cx: &mut DrawCx, canvas: &mut Canvas) {
        let (width, height) = (canvas.width(), canvas.height());
        let value = (self.value)();
        let reading = match &self.readout {
            Some(readout) => readout(),
            None => self.number(value),
        };
        self.shown = Some(match &self.unit {
            Some(unit) => format!("{reading} {unit}"),
            None => reading.clone(),
        });
        if width == 0 || height == 0 {
            return;
        }
        let fraction = self.fraction(value);
        let fill = self.fill_style(cx, value);
        let track = look(cx, &["dial.track", "muted"], "dim");
        let label = look(cx, &["dial.label", "muted"], "dim");
        let caption_style = look(cx, &["dial.caption"], "bold");
        let caption_rows = u16::from(self.caption.is_some());
        let label_rows = u16::from(self.ends);
        let arc_rows = height.saturating_sub(caption_rows + label_rows);
        if crate::a11y::text_mode()
            || width < MIN_ARC.0
            || arc_rows + label_rows < MIN_ARC.1
            || arc_rows < 3
        {
            let shown = self.shown.clone().unwrap_or_default();
            self.draw_compact(
                canvas,
                (width, height),
                fraction,
                &shown,
                (&fill, &track, &caption_style),
            );
            return;
        }
        let placed = self.draw_arc(canvas, (width, arc_rows), fraction, (&fill, &track));
        let readout_style = fill.combine(&Style::parse("bold").unwrap_or_default());
        let centre = |text_width: u16| (width.saturating_sub(text_width)) / 2;
        let (inner_w, inner_h) = placed.inner;
        let unit = self.unit.clone().unwrap_or_default();
        let unit_w = rich::cells::cell_len(&unit) as u16;
        // The reading: big digits when they fit inside the arc.
        let big =
            digits_width(&reading).filter(|w| *w <= inner_w && inner_h >= 2 && placed.middle >= 1);
        let mut below = placed.middle + 1;
        if let Some(digits_w) = big {
            let x = centre(digits_w);
            for row in 0..3u16 {
                let mut at = x;
                for c in reading.chars() {
                    let glyph = crate::digits::glyph(c).expect("a glyph");
                    let part = glyph[row as usize];
                    canvas.print(at, placed.middle - 1 + row, part, Some(&readout_style));
                    at += rich::cells::cell_len(part) as u16;
                }
            }
            below = placed.middle + 2;
        } else {
            let reading_w = rich::cells::cell_len(&reading) as u16;
            if !unit.is_empty() && reading_w + 1 + unit_w <= inner_w.max(reading_w) {
                let x = centre(reading_w + 1 + unit_w);
                canvas.print(x, placed.middle, &reading, Some(&readout_style));
                canvas.print(x + reading_w + 1, placed.middle, &unit, Some(&label));
                below = u16::MAX;
            } else {
                canvas.print(
                    centre(reading_w),
                    placed.middle,
                    &reading,
                    Some(&readout_style),
                );
            }
        }
        if !unit.is_empty() && below < arc_rows + label_rows {
            canvas.print(centre(unit_w), below, &unit, Some(&label));
        }
        if self.ends {
            let (low, high) = (self.number(self.min), self.number(self.max));
            let (low_w, high_w) = (
                rich::cells::cell_len(&low) as u16,
                rich::cells::cell_len(&high) as u16,
            );
            let ((lx, ly), (hx, _)) = placed.ends;
            let row = (ly + 1).min(arc_rows + label_rows - 1);
            let low_x = lx
                .saturating_sub(low_w / 2)
                .min(width.saturating_sub(low_w));
            let high_x = hx
                .saturating_sub(high_w / 2)
                .min(width.saturating_sub(high_w));
            if low_x + low_w < high_x {
                canvas.print(low_x, row, &low, Some(&label));
                canvas.print(high_x, row, &high, Some(&label));
            }
        }
        if let Some(caption) = &self.caption {
            let caption_w = rich::cells::cell_len(caption) as u16;
            canvas.print(centre(caption_w), height - 1, caption, Some(&caption_style));
        }
    }
}

/// Options of a [`dial`]. On any other node they do nothing.
impl Node {
    /// On a [`dial`]: the reading shown and said, instead of the value
    /// with as many decimals as the range calls for (none for a range of
    /// 100 or more, one for 10 or more, else two). A closure that may read
    /// signals: `move || format!("{:.2}", speed.get())`.
    pub fn readout(self, readout: impl Fn() -> String + 'static) -> Node {
        self.widget_mut(|dial: &mut Dial| dial.readout = Some(Box::new(readout)));
        self
    }

    /// On a [`dial`]: the unit after the reading (`Mbps`, `°C`), drawn
    /// under it and said with it.
    pub fn unit(self, unit: impl Into<String>) -> Node {
        let unit = unit.into();
        self.widget_mut(|dial: &mut Dial| dial.unit = Some(unit));
        self
    }

    /// On a [`dial`]: words drawn under it (`Download`), and its accessible
    /// [label](Node::label).
    pub fn caption(self, caption: impl Into<String>) -> Node {
        let caption = caption.into();
        let named = caption.clone();
        self.widget_mut(|dial: &mut Dial| dial.caption = Some(caption));
        self.label(named)
    }

    /// On a [`dial`]: how far round the arc goes, in degrees (270 by
    /// default; 180 is a half circle, 360 a ring), open at the bottom.
    pub fn sweep(self, degrees: f64) -> Node {
        self.widget_mut(|dial: &mut Dial| dial.sweep = degrees.clamp(30.0, 360.0));
        self
    }

    /// On a [`dial`]: colours by where the value is, as a browser draws
    /// an HTML `<meter>` with `low`, `high` and `optimum`: the region the
    /// optimum is in is the theme's `good`, the next `warn`, the far one
    /// `bad`. Below `low` is the low region, above `high` the high one.
    pub fn bands(self, low: f64, high: f64, optimum: f64) -> Node {
        let zone = |upto: f64, inclusive: bool, style: &str| Zone {
            upto,
            inclusive,
            style: style.to_string(),
        };
        let zones = if optimum < low {
            ["good", "warn", "bad"]
        } else if optimum > high {
            ["bad", "warn", "good"]
        } else {
            ["warn", "good", "warn"]
        };
        let zones = vec![
            zone(low, false, zones[0]),
            zone(high, true, zones[1]),
            zone(f64::INFINITY, true, zones[2]),
        ];
        self.widget_mut(|dial: &mut Dial| dial.zones = zones);
        self
    }

    /// On a [`dial`]: colours by thresholds, each `(up to, style)`: a value
    /// up to and including the first bound takes its style (a theme
    /// style's name, `"good"`, or a style, `"bold red"`), up to the next
    /// the next one's, and a value past the last bound the last style.
    ///
    /// ```
    /// use intuituive::prelude::*;
    /// use intuituive::dial;
    ///
    /// let temperature = dial(|| 82.0, 0.0..=120.0)
    ///     .unit("°C")
    ///     .zones([(60.0, "good"), (90.0, "warn"), (120.0, "bad")]);
    /// # let _ = temperature;
    /// ```
    pub fn zones<'a>(self, zones: impl IntoIterator<Item = (f64, &'a str)>) -> Node {
        let mut zones: Vec<Zone> = zones
            .into_iter()
            .map(|(upto, style)| Zone {
                upto,
                inclusive: true,
                style: style.to_string(),
            })
            .collect();
        zones.sort_by(|a, b| a.upto.total_cmp(&b.upto));
        self.widget_mut(|dial: &mut Dial| dial.zones = zones);
        self
    }

    /// On a [`dial`]: how the arc is drawn: [`PixelMode::Braille`] (the
    /// default: eight dots a cell, the roundest) or
    /// [`PixelMode::HalfBlocks`] (two square pixels a cell, solid; any
    /// other mode draws half blocks too).
    pub fn arc_mode(self, mode: PixelMode) -> Node {
        self.widget_mut(|dial: &mut Dial| dial.mode = mode);
        self
    }

    /// On a [`dial`]: whether the range's two ends are drawn under the
    /// arc's ends (yes by default).
    pub fn range_labels(self, on: bool) -> Node {
        self.widget_mut(|dial: &mut Dial| dial.ends = on);
        self
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn bare() -> Dial {
        Dial {
            value: Box::new(|| 0.0),
            min: 0.0,
            max: 1000.0,
            sweep: 270.0,
            readout: None,
            unit: None,
            caption: None,
            zones: Vec::new(),
            mode: PixelMode::Braille,
            ends: true,
            shown: None,
        }
    }

    #[test]
    fn numbers_take_the_decimals_their_range_calls_for() {
        let mut dial = bare();
        assert_eq!(dial.number(287.44), "287");
        dial.max = 50.0;
        assert_eq!(dial.number(12.34), "12.3");
        dial.max = 1.0;
        assert_eq!(dial.number(0.456), "0.46");
    }

    #[test]
    fn a_value_outside_the_range_fills_none_or_all_of_it() {
        let dial = bare();
        assert_eq!(dial.fraction(-5.0), 0.0);
        assert_eq!(dial.fraction(5000.0), 1.0);
        assert_eq!(dial.fraction(f64::NAN), 0.0);
        assert_eq!(dial.fraction(250.0), 0.25);
    }

    #[test]
    fn a_270_degree_arc_reaches_below_its_centre() {
        let mut dial = bare();
        assert!((dial.below() - std::f64::consts::FRAC_1_SQRT_2).abs() < 1e-9);
        dial.sweep = 180.0;
        assert!(dial.below().abs() < 1e-9);
        dial.sweep = 360.0;
        assert_eq!(dial.below(), 1.0);
    }

    #[test]
    fn every_reading_glyph_has_a_width() {
        assert_eq!(digits_width("287.4"), Some(13));
        assert_eq!(digits_width("n/a"), None);
    }
}
