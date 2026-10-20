//! Plain-text cells: data into a core [`Table`] or [`Tree`] without markup.
//!
//! The core takes a `&str` cell as console markup, as upstream takes a `str`:
//! `"[bold]x[/]"` renders a bold `x`, and a stray `[` in a file name, a
//! column name or user input can restyle, link or hide what follows. Data
//! you do not control belongs in a [`Text`], which is never parsed; these
//! extension traits wrap each cell for you.
//!
//! ```
//! use rich::{Console, Table};
//! use rich_ext::table::PlainRows;
//!
//! let mut table = Table::new();
//! table.add_plain_column("[name]");
//! table.add_plain_row(["[bold]x[/]"]);
//! table.add_plain_rows([["a:smile:b"], ["[link=https://x.test]y"]]);
//!
//! let out = Console::builder().width(40).build().render_export(&table);
//! assert!(out.contains("│ [bold]x[/] "));
//! assert!(out.contains("a:smile:b"));
//! assert!(out.contains("[link=https://x.test]y"));
//! assert!(out.contains("┃ [name]"));
//! ```
//!
//! A plain cell is a [`Text`] cell, so it follows upstream's rules for one:
//! it is not highlighted even in a column with `highlight` on, and emoji
//! codes stay as written. For [`Columns`](rich::Columns), build the items with
//! `Columns::from_cells` and `Cell::from(Text::new(item))`.

use rich::{Justify, Table, Text, Tree};

/// Plain-text rows, headers and footers for a core [`Table`].
pub trait PlainRows {
    /// Add a row whose cells are taken literally, never as markup.
    fn add_plain_row<I, S>(&mut self, cells: I) -> &mut Self
    where
        I: IntoIterator<Item = S>,
        S: AsRef<str>;

    /// Add each row of `rows` with [`add_plain_row`](Self::add_plain_row).
    fn add_plain_rows<R, I, S>(&mut self, rows: R) -> &mut Self
    where
        R: IntoIterator<Item = I>,
        I: IntoIterator<Item = S>,
        S: AsRef<str>;

    /// Add a left-justified column whose header is taken literally.
    fn add_plain_column(&mut self, header: impl AsRef<str>) -> &mut Self {
        self.add_plain_column_justify(header, Justify::Left)
    }

    /// Add a column with `justify` whose header is taken literally.
    fn add_plain_column_justify(&mut self, header: impl AsRef<str>, justify: Justify) -> &mut Self;

    /// Set the last column's footer, taken literally.
    fn plain_column_footer(&mut self, footer: impl AsRef<str>) -> &mut Self;
}

impl PlainRows for Table {
    fn add_plain_row<I, S>(&mut self, cells: I) -> &mut Self
    where
        I: IntoIterator<Item = S>,
        S: AsRef<str>,
    {
        self.add_row_text(
            cells
                .into_iter()
                .map(|cell| Text::new(cell.as_ref()))
                .collect(),
        )
    }

    fn add_plain_rows<R, I, S>(&mut self, rows: R) -> &mut Self
    where
        R: IntoIterator<Item = I>,
        I: IntoIterator<Item = S>,
        S: AsRef<str>,
    {
        for row in rows {
            self.add_plain_row(row);
        }
        self
    }

    fn add_plain_column_justify(&mut self, header: impl AsRef<str>, justify: Justify) -> &mut Self {
        self.add_column_text(Text::new(header.as_ref()), justify)
    }

    fn plain_column_footer(&mut self, footer: impl AsRef<str>) -> &mut Self {
        self.column_footer(Text::new(footer.as_ref()))
    }
}

/// Plain-text labels for a core [`Tree`].
///
/// ```
/// use rich::{Console, Tree};
/// use rich_ext::table::PlainTree;
///
/// let mut tree = Tree::new(rich::Text::new("[root]"));
/// tree.add_plain("[b]child");
/// let out = Console::builder().width(30).build().render_export(&tree);
/// assert_eq!(out, "[root]\n└── [b]child\n");
/// ```
pub trait PlainTree {
    /// Add a child whose label is taken literally, returning it.
    fn add_plain(&mut self, label: impl AsRef<str>) -> &mut Tree;
}

impl PlainTree for Tree {
    fn add_plain(&mut self, label: impl AsRef<str>) -> &mut Tree {
        self.add(Text::new(label.as_ref()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rich::Console;

    fn render(table: &Table) -> String {
        Console::builder()
            .width(60)
            .force_terminal(true)
            .build()
            .render_to_string(table)
    }

    #[test]
    fn markup_stays_literal() {
        let mut plain = Table::new();
        plain.add_plain_column("h");
        plain.add_plain_row(["[bold]x[/]"]);
        let out = render(&plain);
        assert!(out.contains("[bold]x[/]"), "{out:?}");
        // The body row carries no style at all.
        let row = out.lines().find(|line| line.contains("[bold]")).unwrap();
        assert_eq!(row, "│ [bold]x[/] │", "{out:?}");

        // The core `add_row` parses the same string.
        let mut markup = Table::new();
        markup.add_column("h");
        markup.add_row(&["[bold]x[/]"]);
        let out = render(&markup);
        assert!(!out.contains("[bold]"), "{out:?}");
        assert!(out.contains("\x1b[1mx"), "{out:?}");
    }

    #[test]
    fn rows_headers_and_footers() {
        let mut table = Table::new().show_footer(true);
        table.add_plain_column_justify("[n]", Justify::Right);
        table.plain_column_footer("[sum]");
        let rows: Vec<Vec<String>> = vec![vec!["[1]".into()], vec!["[/]".into()]];
        table.add_plain_rows(&rows);
        assert_eq!(table.row_count(), 2);
        let out = Console::builder().width(20).build().render_export(&table);
        assert_eq!(
            out,
            "┏━━━━━━━┓\n┃   [n] ┃\n┡━━━━━━━┩\n│   [1] │\n│   [/] │\n├───────┤\n│ [sum] │\n└───────┘\n"
        );
    }

    #[test]
    fn tree_labels_stay_literal() {
        let mut tree = Tree::new(Text::new("r"));
        tree.add_plain("[red]a").add_plain(":smile:");
        let out = Console::builder().width(30).build().render_export(&tree);
        assert_eq!(out, "r\n└── [red]a\n    └── :smile:\n");
    }
}
