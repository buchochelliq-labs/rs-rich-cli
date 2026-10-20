//! Persistence, as an analogue scope's phosphor: the last few frames of a
//! trace drawn together, each older one fainter.

use rich::Color;

use super::{Dataset, GraphType, Marker};

/// The last frames of a trace (an oscilloscope's, a vectorscope's) as
/// [`Dataset`]s for a [`Chart`](super::Chart), the newest in its colour
/// and each older one fainter, as an analogue scope's phosphor fades
/// (xoscope's and glscopeclient's persistence). The app keeps the frames,
/// oldest first (intuiTUIve's `recent(signal, n)` keeps them so), and
/// draws them with [`Layering::PerDataset`](super::Layering::PerDataset)
/// so the newest is on top.
///
/// The colour fades towards the [`background`](Self::background) (black
/// by default) in equal steps down to [`fade`](Self::fade) of its
/// brightness for the oldest frame. A style naming no colour (a theme
/// key) is not faded: every frame takes it.
///
/// ```
/// use rich_ext::chart::{Axis, Chart, Layering, Persistence};
///
/// let frames = vec![
///     vec![(0.0, 0.0), (1.0, 1.0)],
///     vec![(0.0, 0.5), (1.0, 0.5)],
///     vec![(0.0, 1.0), (1.0, 0.0)],
/// ];
/// let trace = Persistence::new(frames).style("#00ff00").fade(0.2);
/// assert_eq!(trace.styles(), ["#003300", "#009900", "#00ff00"]);
/// let chart = Chart::new(trace.datasets())
///     .x_axis(Axis::default().bounds(0.0, 1.0))
///     .y_axis(Axis::default().bounds(0.0, 1.0))
///     .layering(Layering::PerDataset);
/// assert_eq!(chart.datasets().len(), 3);
/// ```
#[derive(Clone, Debug, PartialEq)]
#[must_use = "builder methods return a new value; it does nothing until it is rendered or used"]
pub struct Persistence {
    frames: Vec<Vec<(f64, f64)>>,
    style: String,
    background: Option<Color>,
    floor: f64,
    graph_type: GraphType,
    marker: Marker,
}

impl Persistence {
    /// The frames, oldest first, each the points of one trace.
    pub fn new<F>(frames: impl IntoIterator<Item = F>) -> Self
    where
        F: IntoIterator<Item = (f64, f64)>,
    {
        Persistence {
            frames: frames
                .into_iter()
                .map(|f| f.into_iter().collect())
                .collect(),
            style: "green".to_string(),
            background: None,
            floor: 0.25,
            graph_type: GraphType::Line,
            marker: Marker::Braille,
        }
    }

    /// The newest frame's style: a definition with a colour (`"green"`,
    /// `"bold #33ff66"`), whose colour the older frames fade; or a theme
    /// key, which every frame takes (default `"green"`, a phosphor's).
    pub fn style(mut self, style: impl Into<String>) -> Self {
        self.style = style.into();
        self
    }

    /// The colour the frames fade towards (default black): the terminal's
    /// background, so the oldest frames sink into it. One that does not
    /// parse is ignored.
    pub fn background(mut self, colour: &str) -> Self {
        self.background = Color::parse(colour).ok();
        self
    }

    /// How much of its colour the oldest frame keeps, from 0 (the
    /// background) to 1 (no fading); default a quarter.
    pub fn fade(mut self, floor: f64) -> Self {
        if !floor.is_nan() {
            self.floor = floor.clamp(0.0, 1.0);
        }
        self
    }

    /// How each frame joins its points (default [`GraphType::Line`]).
    pub fn graph_type(mut self, graph_type: GraphType) -> Self {
        self.graph_type = graph_type;
        self
    }

    /// The marker every frame is drawn with (default [`Marker::Braille`]).
    pub fn marker(mut self, marker: Marker) -> Self {
        self.marker = marker;
        self
    }

    /// How many frames it holds.
    pub fn len(&self) -> usize {
        self.frames.len()
    }

    /// Whether it holds none.
    pub fn is_empty(&self) -> bool {
        self.frames.is_empty()
    }

    /// Each frame's style, oldest first.
    pub fn styles(&self) -> Vec<String> {
        let n = self.frames.len();
        (0..n)
            .map(|i| {
                // From the floor (the oldest) up to 1 (the newest).
                let keep = if n < 2 {
                    1.0
                } else {
                    self.floor + (1.0 - self.floor) * i as f64 / (n - 1) as f64
                };
                self.faded(keep)
            })
            .collect()
    }

    /// The frames as datasets, oldest first, each in its faded style.
    pub fn datasets(&self) -> Vec<Dataset> {
        self.frames
            .iter()
            .zip(self.styles())
            .map(|(points, style)| {
                Dataset::new(points.iter().copied())
                    .graph_type(self.graph_type)
                    .marker(self.marker)
                    .style(style)
            })
            .collect()
    }

    /// The style with its colour `keep` of the way from the background.
    fn faded(&self, keep: f64) -> String {
        let background = self
            .background
            .as_ref()
            .and_then(Color::get_truecolor)
            .unwrap_or(rich::ColorTriplet::new(0, 0, 0));
        let mut words: Vec<String> = Vec::new();
        let mut faded = false;
        let mut after_on = false;
        for word in self.style.split_whitespace() {
            let colour = (!faded && !after_on)
                .then(|| Color::parse(word).ok())
                .flatten()
                .and_then(|c| c.get_truecolor());
            match colour {
                Some(c) => {
                    let mix = |a: u8, b: u8| {
                        (f64::from(b) + (f64::from(a) - f64::from(b)) * keep).round() as u8
                    };
                    let (r, g, b) = (
                        mix(c.red, background.red),
                        mix(c.green, background.green),
                        mix(c.blue, background.blue),
                    );
                    words.push(rich::ColorTriplet::new(r, g, b).hex());
                    faded = true;
                }
                None => words.push(word.to_string()),
            }
            after_on = word.eq_ignore_ascii_case("on");
        }
        words.join(" ")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn older_frames_fade_towards_the_background() {
        let p = Persistence::new([[(0.0, 0.0)], [(1.0, 1.0)]])
            .style("bold #ffffff on blue")
            .background("#000000")
            .fade(0.5);
        // Only the foreground colour fades; the rest is kept.
        assert_eq!(p.styles(), ["bold #808080 on blue", "bold #ffffff on blue"]);
        let light = Persistence::new([[(0.0, 0.0)], [(1.0, 1.0)]])
            .style("#000000")
            .background("white")
            .fade(0.0);
        // A named colour through the standard palette: white is #c0c0c0.
        assert_eq!(light.styles(), ["#c0c0c0", "#000000"]);
    }

    #[test]
    fn a_theme_key_is_not_faded_and_one_frame_is_whole() {
        let keyed = Persistence::new([[(0.0, 0.0)], [(1.0, 1.0)]]).style("chart.series.2");
        assert_eq!(keyed.styles(), ["chart.series.2", "chart.series.2"]);
        let one = Persistence::new([[(0.0, 0.0)]]).style("#123456");
        assert_eq!(one.styles(), ["#123456"]);
        assert!(Persistence::new(Vec::<Vec<(f64, f64)>>::new()).is_empty());
    }

    #[test]
    fn datasets_keep_the_frames_in_order() {
        let p = Persistence::new([vec![(0.0, 1.0)], vec![(0.0, 2.0), (1.0, 2.0)]])
            .graph_type(GraphType::Scatter)
            .marker(Marker::Dot);
        let sets = p.datasets();
        assert_eq!(p.len(), 2);
        assert_eq!(sets[0].points(), [(0.0, 1.0)]);
        assert_eq!(sets[1].points(), [(0.0, 2.0), (1.0, 2.0)]);
    }
}
