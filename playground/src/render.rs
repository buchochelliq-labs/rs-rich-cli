//! The kinds of tier-1 block: each turns a text input and its options into
//! renderables printed on a console, through rs-rich's and rs-rich-ext's
//! public APIs only.

use rich::protocol::Renderable;
use rich::{Cell, Columns, Console, Json, Panel, Rule, Style, Table, Text, Tree};
use rich_ext::chart::{BarChart, Charset, Histogram, Orientation, Sparkline};

use crate::options::Options;

/// A kind of block: its name (`data-kind`), whether it needs the full
/// module (syntect), and what its input is.
pub struct Kind {
    pub name: &'static str,
    pub full: bool,
    pub input: &'static str,
}

/// The line that separates the old text from the new in `diff` and
/// `data-diff` inputs, as in a merge conflict.
pub const SEPARATOR: &str = "=======";

pub const KINDS: &[Kind] = &[
    Kind {
        name: "markup",
        full: false,
        input: "console markup, one print per line",
    },
    Kind {
        name: "panel",
        full: false,
        input: "console markup inside a Panel",
    },
    Kind {
        name: "rule",
        full: false,
        input: "one Rule per line, its title in markup",
    },
    Kind {
        name: "columns",
        full: false,
        input: "one item per line, laid out in Columns",
    },
    Kind {
        name: "table",
        full: false,
        input: "CSV: a header line, then rows; cells are markup",
    },
    Kind {
        name: "tree",
        full: false,
        input: "an indented outline; labels are markup",
    },
    Kind {
        name: "json",
        full: false,
        input: "a JSON document",
    },
    Kind {
        name: "sparkline",
        full: false,
        input: "numbers",
    },
    Kind {
        name: "bars",
        full: false,
        input: "label,value lines",
    },
    Kind {
        name: "histogram",
        full: false,
        input: "numbers",
    },
    Kind {
        name: "diff",
        full: false,
        input: "the old text, a ======= line, the new text",
    },
    Kind {
        name: "data",
        full: false,
        input: "a JSON or YAML document",
    },
    Kind {
        name: "data-diff",
        full: false,
        input: "two JSON or YAML documents, split by =======",
    },
    Kind {
        name: "syntax",
        full: true,
        input: "source code (data-lang names its language)",
    },
    Kind {
        name: "markdown",
        full: true,
        input: "Markdown",
    },
];

pub fn kind(name: &str) -> Option<&'static Kind> {
    KINDS.iter().find(|kind| kind.name == name)
}

fn style(spec: &str) -> Result<Style, String> {
    Style::parse(spec).map_err(|e| format!("style {spec:?}: {e}"))
}

fn markup(text: &str) -> Result<Text, String> {
    Text::from_markup(text).map_err(|e| e.to_string())
}

/// Print `input` as a block of kind `name` on `console`.
pub fn draw(console: &Console, name: &str, input: &str, options: &Options) -> Result<(), String> {
    match name {
        "markup" => {
            for line in input.lines() {
                console.try_print_str(line).map_err(|e| e.to_string())?;
            }
            Ok(())
        }
        "panel" => print(console, &panel(input, options)?),
        "rule" => {
            // An empty input is one plain line.
            let lines: Vec<&str> = if input.is_empty() {
                vec![""]
            } else {
                input.lines().collect()
            };
            for line in lines {
                let mut rule = if line.trim().is_empty() {
                    Rule::line()
                } else {
                    Rule::new(line)
                };
                if let Some(align) = options.align()? {
                    rule = rule.align(align);
                }
                if let Some(characters) = &options.characters {
                    rule = rule.characters(characters.as_str());
                }
                if let Some(spec) = &options.style {
                    rule = rule.style(style(spec)?);
                }
                console.print(&rule);
            }
            Ok(())
        }
        "columns" => {
            let items = input
                .lines()
                .filter(|l| !l.trim().is_empty())
                .map(|l| markup(l.trim()));
            let cells = items
                .map(|item| item.map(Cell::from))
                .collect::<Result<Vec<_>, _>>()?;
            let mut columns = Columns::from_cells(cells);
            if let Some(expand) = options.expand {
                columns = columns.expand(expand);
            }
            if let Some(title) = &options.title {
                columns = columns.title(title.as_str());
            }
            print(console, &columns)
        }
        "table" => print(console, &table(input, options)?),
        "tree" => {
            let tree = tree(input, options)?;
            match &options.title {
                Some(title) => print(console, &Panel::fit(Box::new(tree)).title(title.as_str())),
                None => print(console, &tree),
            }
        }
        "json" => {
            let mut json_options = rich::json::JsonOptions::default();
            if let Some(indent) = options.indent {
                json_options.indent = Some(" ".repeat(indent));
            }
            json_options.sort_keys = options.sort_keys.unwrap_or(false);
            let json = Json::with_options(input, &json_options).map_err(|e| e.to_string())?;
            print(console, &json)
        }
        "sparkline" => {
            let mut chart = Sparkline::new(numbers(input)?);
            if let Some(min_max) = options.min_max {
                chart = chart.min_max(min_max);
            }
            if let Some(threshold) = options.threshold {
                chart = chart.threshold(threshold);
            }
            if options.charset.as_deref() == Some("braille") {
                chart = chart.charset(Charset::Braille);
            }
            print(console, &chart)
        }
        "bars" => {
            let mut chart = BarChart::from_pairs(pairs(input)?);
            if let Some(orientation) = orientation(options)? {
                chart = chart.orientation(orientation);
            }
            if let Some(width) = options.bar_width {
                chart = chart.bar_width(width.min(200));
            }
            print(console, &chart)
        }
        "histogram" => {
            let mut chart = Histogram::new(numbers(input)?);
            if let Some(bins) = options.bins {
                chart = chart.bins(bins.clamp(1, 100));
            }
            if let Some(orientation) = orientation(options)? {
                chart = chart.orientation(orientation);
            }
            if let Some(width) = options.bar_width {
                chart = chart.bar_width(width.min(200));
            }
            print(console, &chart)
        }
        "diff" => {
            use rich_ext::diff::{DiffView, Layout};
            let (old, new) = split(input)?;
            let mut view = DiffView::new(old, new);
            if let Some(layout) = &options.layout {
                view = view.layout(match layout.as_str() {
                    "unified" => Layout::Unified,
                    "side-by-side" => Layout::SideBySide,
                    other => return Err(format!("unknown layout {other:?}")),
                });
            }
            if let Some(context) = options.context {
                view = view.context(context);
            }
            if options.old_title.is_some() || options.new_title.is_some() {
                view = view.titles(
                    options.old_title.clone().unwrap_or_default(),
                    options.new_title.clone().unwrap_or_default(),
                );
            }
            print(console, &view)
        }
        "data" => {
            use rich_ext::data::{Explorer, FlatView, View};
            let doc = document(input, options)?;
            match options.view.as_deref().unwrap_or("tree") {
                "flat" => print(
                    console,
                    &FlatView::new(&doc).show_types(options.show_types.unwrap_or(false)),
                ),
                view => {
                    let mut explorer = Explorer::new(&doc)
                        .view(match view {
                            "tree" => View::Tree,
                            "table" => View::Table,
                            other => return Err(format!("unknown view {other:?}")),
                        })
                        .show_types(options.show_types.unwrap_or(false));
                    if let Some(label) = &options.root_label {
                        explorer = explorer.root_label(label.as_str());
                    }
                    print(console, &explorer)
                }
            }
        }
        "data-diff" => {
            let (old, new) = split(input)?;
            let old = document(old, options)?;
            let new = document(new, options)?;
            print(console, &rich_ext::data::DiffView::new(&old, &new))
        }
        #[cfg(feature = "full")]
        "syntax" => {
            let lang = options.lang.as_deref().unwrap_or("text");
            let mut syntax = rich::Syntax::new(input, lang);
            if let Some(theme) = &options.theme {
                syntax = syntax.theme(theme.as_str());
            }
            if let Some(on) = options.line_numbers {
                syntax = syntax.line_numbers(on);
            }
            if let Some(on) = options.word_wrap {
                syntax = syntax.word_wrap(on);
            }
            if let Some(padding) = &options.padding {
                syntax = syntax.padding_sides(padding.unpack()?);
            }
            if let Some(tab_size) = options.tab_size {
                syntax = syntax.tab_size(tab_size.clamp(1, 16));
            }
            print(console, &syntax)
        }
        #[cfg(feature = "full")]
        "markdown" => {
            let mut markdown = rich::markdown::Markdown::new(input);
            if let Some(on) = options.hyperlinks {
                markdown = markdown.hyperlinks(on);
            }
            if let Some(justify) = options.justify()? {
                markdown = markdown.justify(justify);
            }
            if let Some(spec) = &options.style {
                markdown = markdown.style(style(spec)?);
            }
            if let Some(theme) = &options.code_theme {
                markdown = markdown.code_theme(theme.as_str());
            }
            if let Some(lexer) = &options.inline_code_lexer {
                markdown = markdown.inline_code_lexer(lexer.as_str());
            }
            print(console, &markdown)
        }
        other => match kind(other) {
            Some(_) => Err(format!("{other:?} needs the full module")),
            None => Err(format!("unknown kind {other:?}")),
        },
    }
}

fn print(console: &Console, renderable: &dyn Renderable) -> Result<(), String> {
    console.print(renderable);
    Ok(())
}

fn panel(input: &str, options: &Options) -> Result<Panel, String> {
    let mut text = markup(input.trim_end_matches('\n'))?;
    if let Some(justify) = options.justify()? {
        text = text.justify(justify);
    }
    if let Some(overflow) = options.overflow()? {
        text = text
            .overflow(overflow)
            .no_wrap(overflow != rich::Overflow::Fold);
    }
    let mut panel = Panel::new(Box::new(text)).expand(options.expand.unwrap_or(true));
    if let Some(title) = &options.title {
        panel = panel.title(title.as_str());
    }
    if let Some(subtitle) = &options.subtitle {
        panel = panel.subtitle(subtitle.as_str());
    }
    if let Some(align) = options.align()? {
        panel = panel.title_align(align);
    }
    if let Some(set) = options.box_set()? {
        panel = panel.box_set(set);
    }
    if let Some(spec) = &options.border_style {
        panel = panel.border_style(style(spec)?);
    }
    if let Some(padding) = &options.padding {
        panel = panel.padding(padding.unpack()?);
    }
    Ok(panel)
}

fn table(input: &str, options: &Options) -> Result<Table, String> {
    let mut rows = input.lines().filter(|l| !l.trim().is_empty()).map(csv_line);
    let header = rows.next().ok_or("a table needs a header line")?;
    let mut table = Table::new();
    if let Some(title) = &options.title {
        table = table.title(title.as_str());
    }
    if let Some(caption) = &options.caption {
        table = table.caption(caption.as_str());
    }
    if let Some(set) = options.box_set()? {
        table = table.box_set(set);
    }
    if let Some(spec) = &options.border_style {
        table = table.border_style(style(spec)?);
    }
    if let Some(on) = options.show_lines {
        table = table.show_lines(on);
    }
    if let Some(on) = options.show_header {
        table = table.show_header(on);
    }
    if let Some(on) = options.show_edge {
        table = table.show_edge(on);
    }
    if let Some(on) = options.expand {
        table = table.expand(on);
    }
    // `justify` lists one value per column ("left,left,right").
    let justify: Vec<&str> = options
        .justify
        .as_deref()
        .map(|j| j.split(',').collect())
        .unwrap_or_default();
    for (index, name) in header.iter().enumerate() {
        match justify
            .get(index)
            .map(|j| j.trim())
            .filter(|j| !j.is_empty())
        {
            Some(j) => table.add_column_justify(name.as_str(), crate::options::justify(j)?),
            None => table.add_column(name.as_str()),
        };
    }
    for row in rows {
        let cells = row
            .iter()
            .map(|cell| markup(cell))
            .collect::<Result<Vec<_>, _>>()?;
        table.add_row_text(cells);
    }
    Ok(table)
}

/// One CSV line: commas split fields, double quotes group them (`""` is a
/// quote inside one), and spaces around a field go.
pub fn csv_line(line: &str) -> Vec<String> {
    let mut fields = Vec::new();
    let mut field = String::new();
    let mut quoted = false;
    let mut chars = line.chars().peekable();
    while let Some(c) = chars.next() {
        match c {
            '"' if quoted && chars.peek() == Some(&'"') => {
                chars.next();
                field.push('"');
            }
            '"' if quoted => quoted = false,
            '"' if field.trim().is_empty() => {
                field.clear();
                quoted = true;
            }
            ',' if !quoted => fields.push(std::mem::take(&mut field).trim().to_string()),
            _ => field.push(c),
        }
    }
    fields.push(field.trim().to_string());
    fields
}

/// An outline: each line a label, its indent (spaces, a tab counting four)
/// placing it under the nearest less-indented line above.
fn tree(input: &str, options: &Options) -> Result<Tree, String> {
    struct Node {
        label: String,
        children: Vec<Node>,
    }
    let mut lines = input.lines().filter(|l| !l.trim().is_empty());
    let root_line = lines.next().ok_or("a tree needs a root line")?;
    let mut root = Node {
        label: root_line.trim().to_string(),
        children: Vec::new(),
    };
    // The path of child indices from the root to the last line, with indents.
    let mut path: Vec<(usize, usize)> = Vec::new();
    for line in lines {
        let indent = line
            .chars()
            .take_while(|c| c.is_whitespace())
            .map(|c| if c == '\t' { 4 } else { 1 })
            .sum();
        while path.last().is_some_and(|&(depth, _)| depth >= indent) {
            path.pop();
        }
        let mut parent = &mut root;
        for &(_, index) in &path {
            parent = &mut parent.children[index];
        }
        parent.children.push(Node {
            label: line.trim().to_string(),
            children: Vec::new(),
        });
        path.push((indent, parent.children.len() - 1));
    }
    fn attach(tree: &mut Tree, node: &Node) -> Result<(), String> {
        let child = tree.add(markup(&node.label)?);
        for grandchild in &node.children {
            attach(child, grandchild)?;
        }
        Ok(())
    }
    let mut tree = Tree::new(markup(&root.label)?);
    if let Some(spec) = &options.guide_style {
        tree = tree.guide_style(style(spec)?);
    }
    for child in &root.children {
        attach(&mut tree, child)?;
    }
    Ok(tree)
}

fn numbers(input: &str) -> Result<Vec<f64>, String> {
    input
        .split(|c: char| c.is_whitespace() || c == ',')
        .filter(|s| !s.is_empty())
        .map(|s| s.parse::<f64>().map_err(|_| format!("not a number: {s:?}")))
        .collect()
}

fn pairs(input: &str) -> Result<Vec<(String, f64)>, String> {
    input
        .lines()
        .filter(|l| !l.trim().is_empty())
        .map(|line| {
            let fields = csv_line(line);
            match fields.as_slice() {
                [label, value] => value
                    .parse::<f64>()
                    .map(|v| (label.clone(), v))
                    .map_err(|_| format!("not a number: {value:?}")),
                _ => Err(format!("expected label,value: {line:?}")),
            }
        })
        .collect()
}

fn orientation(options: &Options) -> Result<Option<Orientation>, String> {
    options
        .orientation
        .as_deref()
        .map(|name| match name {
            "horizontal" => Ok(Orientation::Horizontal),
            "vertical" => Ok(Orientation::Vertical),
            other => Err(format!("unknown orientation {other:?}")),
        })
        .transpose()
}

fn split(input: &str) -> Result<(&str, &str), String> {
    let mut offset = 0;
    for line in input.split_inclusive('\n') {
        if line.trim_end() == SEPARATOR {
            return Ok((&input[..offset], &input[offset + line.len()..]));
        }
        offset += line.len();
    }
    Err(format!(
        "put a {SEPARATOR} line between the old text and the new"
    ))
}

fn document(input: &str, options: &Options) -> Result<rich_ext::data::Node, String> {
    use rich_ext::data::{parse, Format};
    let format = match options.format.as_deref() {
        Some("json") => Format::Json,
        Some("yaml") => Format::Yaml,
        Some(other) => return Err(format!("unknown format {other:?}")),
        None => Format::detect(input, None).ok_or("not JSON or YAML")?,
    };
    parse(format, input).map_err(|e| e.to_string())
}
