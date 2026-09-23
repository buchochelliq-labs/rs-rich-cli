//! Convenience macros and the print helpers they use.
//!
//! - [`rich_table!`](crate::rich_table), [`rich_panel!`](crate::rich_panel),
//!   [`rich_tree!`](crate::rich_tree) and [`rich_progress!`](crate::rich_progress)
//!   build the ordinary core types, so the result can still be configured.
//! - [`rich_dbg!`](crate::rich_dbg) is `dbg!` rendered through `Pretty`.
//! - With the `macros` feature, [`rich_println!`](crate::rich_println),
//!   [`rich_eprintln!`](crate::rich_eprintln) and
//!   [`rich_trace!`](crate::rich_trace) print compile-time checked markup.

use std::io::{IsTerminal, Write};

use rich::{Console, Renderable};

/// A console for standard error: a terminal only when stderr is one.
pub fn stderr_console() -> Console {
    Console::builder()
        .force_terminal(std::io::stderr().is_terminal())
        .build()
}

/// Print `renderable` and a newline to standard error.
pub fn eprint(renderable: &dyn Renderable) {
    let rendered = stderr_console().render_to_string(renderable);
    let mut stderr = std::io::stderr().lock();
    let _ = writeln!(stderr, "{rendered}");
}

/// Print `renderable` to standard output.
pub fn print(renderable: &dyn Renderable) {
    Console::new().print(renderable);
}

/// A table from a header row and cell rows; each cell is anything `Display`.
///
/// ```
/// let table = rich_ext::rich_table!(["Name", "Age"], ["Alice", 30], ["Bob", 4]);
/// # let _ = table;
/// ```
#[macro_export]
macro_rules! rich_table {
    ([$($header:expr),* $(,)?] $(, [$($cell:expr),* $(,)?])* $(,)?) => {{
        let mut table = $crate::__private::rich::Table::new();
        $( table.add_column(::std::string::ToString::to_string(&$header)); )*
        $( table.add_row(&[$( ::std::string::ToString::to_string(&$cell).as_str() ),*]); )*
        table
    }};
}

/// A panel around a renderable, or around markup when given a string literal.
/// Optional `title = …` and `subtitle = …` follow.
///
/// ```
/// let panel = rich_ext::rich_panel!("[bold]ready[/]", title = "status");
/// # let _ = panel;
/// ```
#[macro_export]
macro_rules! rich_panel {
    ($markup:literal $(, $key:ident = $value:expr)* $(,)?) => {
        $crate::rich_panel!(@build ::std::boxed::Box::new(
            $crate::__private::rich::Text::from_markup($markup)
                .unwrap_or_else(|_| $crate::__private::rich::Text::new($markup))
        ) $(, $key = $value)*)
    };
    (@build $content:expr $(, $key:ident = $value:expr)*) => {{
        let panel = $crate::__private::rich::Panel::new($content);
        $( let panel = panel.$key($value); )*
        panel
    }};
    ($content:expr $(, $key:ident = $value:expr)* $(,)?) => {
        $crate::rich_panel!(@build ::std::boxed::Box::new($content) $(, $key = $value)*)
    };
}

/// A tree from nested labels: `rich_tree!("root" => ["a", "b" => ["c"]])`.
///
/// ```
/// let tree = rich_ext::rich_tree!("src" => ["main.rs", "lib" => ["mod.rs"]]);
/// # let _ = tree;
/// ```
#[macro_export]
macro_rules! rich_tree {
    ($label:expr $(=> [$($children:tt)*])?) => {{
        #[allow(unused_mut)]
        let mut tree = $crate::__private::rich::Tree::new($label);
        $( $crate::rich_tree!(@children tree; $($children)*); )?
        tree
    }};
    (@children $parent:ident; ) => {};
    (@children $parent:ident; $label:expr => [$($children:tt)*] $(, $($rest:tt)*)?) => {{
        let node = $parent.add($label);
        $crate::rich_tree!(@children node; $($children)*);
        $( $crate::rich_tree!(@children $parent; $($rest)*); )?
    }};
    (@children $parent:ident; $label:expr $(, $($rest:tt)*)?) => {{
        $parent.add($label);
        $( $crate::rich_tree!(@children $parent; $($rest)*); )?
    }};
}

/// Iterate with a progress bar on standard output: core's `track`.
///
/// ```no_run
/// for _ in rich_ext::rich_progress!(0..10, "Working") {}
/// ```
#[macro_export]
macro_rules! rich_progress {
    ($iter:expr, $description:expr $(,)?) => {
        $crate::__private::rich::track($iter, $description)
    };
    ($iter:expr $(,)?) => {
        $crate::__private::rich::track($iter, "Working...")
    };
}

/// `dbg!` through `Pretty`: prints `[file:line:column] expression = value` to
/// standard error with the value highlighted, and returns the value.
///
/// ```
/// let doubled = rich_ext::rich_dbg!(2 * 21);
/// assert_eq!(doubled, 42);
/// ```
#[macro_export]
macro_rules! rich_dbg {
    () => {
        $crate::macros::eprint(&$crate::__private::rich::Text::styled(
            ::std::format!("[{}:{}:{}]", ::std::file!(), ::std::line!(), ::std::column!()),
            "dim",
        ))
    };
    ($value:expr $(,)?) => {
        match $value {
            value => {
                $crate::macros::eprint(&$crate::macros::dbg_line(
                    ::std::file!(),
                    ::std::line!(),
                    ::std::column!(),
                    ::std::stringify!($value),
                    &value,
                ));
                value
            }
        }
    };
    ($($value:expr),+ $(,)?) => {
        ($($crate::rich_dbg!($value)),+,)
    };
}

/// The line `rich_dbg!` prints: a dim location, the expression, and the
/// value through `Pretty`.
pub fn dbg_line(
    file: &str,
    line: u32,
    column: u32,
    expression: &str,
    value: &dyn std::fmt::Debug,
) -> rich::Text {
    use rich::Highlighter;
    let mut text = rich::Text::styled(format!("[{file}:{line}:{column}] "), "dim");
    text.append(expression, Some("bold".into()));
    text.append(" = ", None);
    let mut rendered = rich::Text::new(format!("{value:#?}"));
    rich::ReprHighlighter::new().highlight(&mut rendered);
    text.append_text(&rendered)
}

/// Print checked markup to standard output: `rich_println!("[bold]{x}[/]")`.
#[cfg(feature = "macros")]
#[macro_export]
macro_rules! rich_println {
    ($($arg:tt)*) => {
        $crate::macros::print(&$crate::richf!($($arg)*))
    };
}

/// Print checked markup to standard error.
#[cfg(feature = "macros")]
#[macro_export]
macro_rules! rich_eprintln {
    ($($arg:tt)*) => {
        $crate::macros::eprint(&$crate::richf!($($arg)*))
    };
}

/// Print checked markup to standard error after a dim `file:line` — a quick
/// trace line. For log pipelines, route `log`/`tracing` through `RichHandler`.
#[cfg(feature = "macros")]
#[macro_export]
macro_rules! rich_trace {
    ($($arg:tt)*) => {
        $crate::macros::eprint(&$crate::__private::rich::Text::styled(
            ::std::format!("{}:{} ", ::std::file!(), ::std::line!()),
            "dim",
        ).append_text(&$crate::richf!($($arg)*)))
    };
}
