//! Config precedence (#413): which layer (defaults, config files,
//! environment, command line) sets each key, and what it overrides.
//!
//! Markers carry the meaning as well as styles: the winning value is marked
//! `✔` and overridden ones `✗` (`*` and `x` on an ASCII-only console), so
//! the output reads the same without colour.

use super::{join_lines, style};
use rich::table::Table;
use rich::{Console, ConsoleOptions, Renderable, Segment, Style, Text};

/// One source of values, such as `defaults`, `project` or `command line`.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Layer {
    pub name: String,
    /// Where the values came from: a file path, `RICH_*`, ...
    pub origin: Option<String>,
    /// `(key, value as displayed)`, in the layer's own order.
    pub values: Vec<(String, String)>,
}

impl Layer {
    pub fn new(name: impl Into<String>) -> Self {
        Layer {
            name: name.into(),
            ..Default::default()
        }
    }
    pub fn origin(mut self, origin: impl Into<String>) -> Self {
        self.origin = Some(origin.into());
        self
    }
    pub fn value(mut self, key: impl Into<String>, value: impl Into<String>) -> Self {
        self.values.push((key.into(), value.into()));
        self
    }
}

/// A key's effective value and the values it overrides.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Resolved {
    pub key: String,
    pub value: String,
    /// The index of the layer that set `value`.
    pub winner: usize,
    /// Overridden `(layer index, value)`s, lowest precedence first.
    pub shadowed: Vec<(usize, String)>,
}

/// Layers of configuration in ascending priority: a later layer wins.
///
/// ```
/// use rich_ext::cli_doc::{Layer, Precedence};
///
/// let precedence = Precedence::new()
///     .layer(Layer::new("defaults").value("width", "80").value("theme", "dark"))
///     .layer(Layer::new("env").origin("RICH_WIDTH").value("width", "100"));
/// let resolved = precedence.resolve();
/// assert_eq!(resolved[0].value, "100");
/// assert_eq!(resolved[0].winner, 1);
/// assert_eq!(resolved[0].shadowed, vec![(0, "80".to_string())]);
/// assert_eq!(resolved[1].value, "dark");
/// ```
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Precedence {
    pub layers: Vec<Layer>,
}

impl Precedence {
    pub fn new() -> Self {
        Precedence::default()
    }

    /// Add a layer above the ones added so far.
    pub fn layer(mut self, layer: Layer) -> Self {
        self.layers.push(layer);
        self
    }

    /// Every key, in first-seen order, with its winner. A later layer wins;
    /// within one layer, a later value for the same key wins.
    pub fn resolve(&self) -> Vec<Resolved> {
        let mut out: Vec<Resolved> = Vec::new();
        for (index, layer) in self.layers.iter().enumerate() {
            for (key, value) in &layer.values {
                match out.iter_mut().find(|r| &r.key == key) {
                    Some(resolved) => {
                        let old = std::mem::replace(&mut resolved.value, value.clone());
                        resolved.shadowed.push((resolved.winner, old));
                        resolved.winner = index;
                    }
                    None => out.push(Resolved {
                        key: key.clone(),
                        value: value.clone(),
                        winner: index,
                        shadowed: Vec::new(),
                    }),
                }
            }
        }
        out
    }

    /// A table of every key across the layers.
    pub fn view(&self) -> PrecedenceView {
        PrecedenceView {
            precedence: self.clone(),
        }
    }

    /// The chain for `key`, highest precedence first, or `None` when no
    /// layer sets it.
    pub fn explain(&self, key: &str) -> Option<Explanation> {
        let resolved = self.resolve().into_iter().find(|r| r.key == key)?;
        Some(Explanation {
            layers: self.layers.clone(),
            resolved,
        })
    }
}

fn markers(c: &Console) -> (&'static str, &'static str) {
    if c.ascii_only() {
        ("*", "x")
    } else {
        ("✔", "✗")
    }
}

/// A table: one row per key, one column per layer, then the effective value.
#[derive(Clone, Debug)]
pub struct PrecedenceView {
    precedence: Precedence,
}

impl Renderable for PrecedenceView {
    fn rich_render(&self, c: &Console, o: &ConsoleOptions) -> Vec<Segment> {
        let (win, lose) = markers(c);
        let winner = style(c, "config.winner");
        let shadowed = style(c, "config.shadowed");
        let key_style = style(c, "config.key");
        let mut table = Table::new();
        table.add_column("Key");
        for layer in &self.precedence.layers {
            table.add_column(layer.name.clone());
        }
        table.add_column("Effective");
        let origins: Vec<String> = self
            .precedence
            .layers
            .iter()
            .filter_map(|l| l.origin.as_ref().map(|o| format!("{}: {o}", l.name)))
            .collect();
        if !origins.is_empty() {
            table = table.caption(origins.join(", "));
        }
        for resolved in self.precedence.resolve() {
            let mut row = vec![Text::styled(resolved.key.clone(), key_style.clone())];
            for index in 0..self.precedence.layers.len() {
                let cell = if index == resolved.winner {
                    Text::styled(format!("{win} {}", resolved.value), winner.clone())
                } else {
                    // A layer's last value for the key is the one it offers.
                    match resolved.shadowed.iter().rev().find(|(i, _)| *i == index) {
                        Some((_, value)) => {
                            Text::styled(format!("{lose} {value}"), shadowed.clone())
                        }
                        None => Text::new(""),
                    }
                };
                row.push(cell);
            }
            row.push(Text::new(resolved.value.clone()));
            table.add_row_text(row);
        }
        join_lines(c.render_lines(&table, o, false))
    }
}

/// How one key was resolved, highest precedence first.
#[derive(Clone, Debug)]
pub struct Explanation {
    layers: Vec<Layer>,
    resolved: Resolved,
}

impl Explanation {
    /// The resolution this explains.
    pub fn resolved(&self) -> &Resolved {
        &self.resolved
    }
}

impl Renderable for Explanation {
    fn rich_render(&self, c: &Console, o: &ConsoleOptions) -> Vec<Segment> {
        let (win, lose) = markers(c);
        let resolved = &self.resolved;
        // (layer index, value), highest precedence first.
        let mut chain: Vec<(usize, &str)> = vec![(resolved.winner, resolved.value.as_str())];
        chain.extend(
            resolved
                .shadowed
                .iter()
                .rev()
                .map(|(i, v)| (*i, v.as_str())),
        );
        let label = |index: usize| {
            let layer = &self.layers[index];
            match &layer.origin {
                Some(origin) => (layer.name.clone(), format!(" ({origin})")),
                None => (layer.name.clone(), String::new()),
            }
        };
        let name_width = chain
            .iter()
            .map(|(i, _)| {
                let (name, origin) = label(*i);
                rich::cells::cell_len(&name) + rich::cells::cell_len(&origin)
            })
            .max()
            .unwrap_or(0);
        let value_width = chain
            .iter()
            .map(|(_, v)| rich::cells::cell_len(v))
            .max()
            .unwrap_or(0);
        let key_style = style(c, "config.key");
        let mut header = Text::new("");
        header.append(&resolved.key, Some(key_style.into()));
        header.append(" = ", None);
        header.append(&resolved.value, Some(style(c, "config.winner").into()));
        let plain = Style::new();
        let mut rows = header.render_lines(c.theme(), &plain, Some(o.max_width.max(1)));
        for (position, (index, value)) in chain.iter().enumerate() {
            let (name, origin) = label(*index);
            let pad = name_width - rich::cells::cell_len(&name) - rich::cells::cell_len(&origin);
            let value_pad = value_width - rich::cells::cell_len(value);
            let mut line = Text::new("  ");
            let (marker, value_style, note) = if position == 0 {
                (win, style(c, "config.winner"), "effective".to_string())
            } else {
                // Overridden by the next layer up the chain.
                let above = &self.layers[chain[position - 1].0].name;
                (
                    lose,
                    style(c, "config.shadowed"),
                    format!("overridden by {above}"),
                )
            };
            line.append(marker, Some(value_style.clone().into()));
            line.append(" ", None);
            line.append(&name, None);
            if !origin.is_empty() {
                line.append(&origin, Some(style(c, "config.origin").into()));
            }
            line.append(&" ".repeat(pad + 2), None);
            line.append(value, Some(value_style.into()));
            line.append(&" ".repeat(value_pad + 2), None);
            line.append(&note, None);
            rows.extend(line.render_lines(c.theme(), &plain, Some(o.max_width.max(1))));
        }
        join_lines(rows)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_repeated_key_within_a_layer_keeps_the_last_value() {
        let precedence = Precedence::new().layer(Layer::new("a").value("k", "1").value("k", "2"));
        let resolved = precedence.resolve();
        assert_eq!(resolved[0].value, "2");
        assert_eq!(resolved[0].shadowed, vec![(0, "1".to_string())]);
    }

    #[test]
    fn explain_is_none_for_an_unknown_key() {
        assert!(Precedence::new()
            .layer(Layer::new("a"))
            .explain("nope")
            .is_none());
    }
}
