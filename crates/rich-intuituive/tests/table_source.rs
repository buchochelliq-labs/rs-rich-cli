//! A table over a source, and a view of one filtered, sorted and searched
//! on a task (round 6).

use std::sync::atomic::{AtomicUsize, Ordering as Atomic};
use std::sync::Arc;

use intuituive::prelude::*;
use intuituive::widgets::{
    source_table, view_of, Column, Order, RowCount, TableOptions, TableSource, ViewQuery,
};

/// A source of `total` rows, `found` of them known yet, read by `len` on
/// the app's thread (`row`, read on a worker, reads no signals); it counts
/// the rows read.
struct Numbers {
    found: Signal<usize>,
    total: usize,
    read: Arc<AtomicUsize>,
}

impl TableSource for Numbers {
    fn len(&self) -> RowCount {
        let found = self.found.get();
        if found >= self.total {
            RowCount::Exact(self.total)
        } else {
            RowCount::AtLeast(found)
        }
    }

    fn row(&self, i: usize) -> Vec<String> {
        self.read.fetch_add(1, Atomic::SeqCst);
        vec![i.to_string(), format!("{}", (i * 7) % 10)]
    }
}

fn plain(driver: &intuituive::Driver) -> Vec<String> {
    driver
        .screen()
        .plain()
        .iter()
        .map(|row| row.trim_end().to_string())
        .collect()
}

#[test]
fn a_view_filters_sorts_and_finds_over_a_growing_source() {
    let read = Arc::new(AtomicUsize::new(0));
    let counted = read.clone();
    let app = App::new(move || {
        let found = signal(50usize);
        let source = Numbers {
            found,
            total: 100,
            read: counted.clone(),
        };
        let view = view_of(source, || {
            ViewQuery::default()
                .keep(|row| row[1] == "3")
                .sort(0, Order::Descending)
                .find(|row| row[0].ends_with('9'))
        });
        let cursor = signal(0usize);
        let columns = vec![Column::new("n", Size::Auto), Column::new("d", Size::Auto)];
        let table = source_table(view, columns, cursor, TableOptions::default())
            .cursor_by(move |i| view.source_row(i));
        column([
            table.fixed(4),
            text(move || {
                let (n, of) = view.place(cursor.get());
                format!("{} rows, {n}/{of}", view.len())
            }),
        ])
        .on_key("n", move |_| {
            if let Some(to) = view.next_found(cursor.get_untracked()) {
                cursor.set(to);
            }
        })
        .on_key("N", move |_| {
            if let Some(to) = view.previous_found(cursor.get_untracked()) {
                cursor.set(to);
            }
        })
        .on_key("x", move |_| found.set(100))
    });
    let mut driver = app.wait_for_tasks(true).driver(16, 5);
    driver.settle();
    // Of the first 50 rows, 9, 19, 29, 39 and 49 end in 3 (×7): newest
    // first, the count not final.
    assert_eq!(plain(&driver)[1..4], ["49 3", "39 3", "29 3"]);
    assert_eq!(plain(&driver)[4], "5+ rows, 1/5");
    assert_eq!(read.load(Atomic::SeqCst) >= 50, true);
    driver.press("n n");
    assert_eq!(plain(&driver)[4], "5+ rows, 3/5");
    driver.press("N N N");
    assert_eq!(plain(&driver)[4], "5+ rows, 5/5");
    // The count is final: a pass over all hundred rows; the cursor stays
    // on row 9.
    driver.press("x");
    assert_eq!(plain(&driver)[4], "10 rows, 10/10");
    driver.press("g");
    assert_eq!(plain(&driver)[1], "99 3");
}

#[test]
fn with_no_query_the_view_is_the_source() {
    let app = App::new(|| {
        let rows: Vec<Vec<String>> = (0..5).map(|i| vec![format!("r{i}")]).collect();
        let view = view_of(rows, ViewQuery::default);
        source_table(
            view,
            vec![Column::new("r", Size::Auto)],
            signal(0),
            TableOptions::default(),
        )
    });
    let mut driver = app.wait_for_tasks(true).driver(6, 6);
    driver.settle();
    assert_eq!(plain(&driver), ["r", "r0", "r1", "r2", "r3", "r4"]);
}
