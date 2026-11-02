//! `markdown`: a Markdown node kept as blocks, which streams (#797).
#![cfg(feature = "syntax")]

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use intuituive::prelude::*;
use intuituive::{MarkdownStream, button, markdown};

/// An answer of about 2 KB, with every kind of block.
fn answer() -> String {
    let mut text = String::from(
        "# Streaming\n\nHere is **bold** and `code` text, with a [link](https://example.com/a).\n\n",
    );
    text.push_str("- one\n- two\n  - nested\n\n1. first\n2. second\n\n> quoted\n> lines\n\n");
    text.push_str("```rust\nfn main() {\n    println!(\"hi\");\n}\n```\n\n");
    text.push_str("| a | b |\n|---|---|\n| 1 | 2 |\n\n");
    while text.len() < 2048 {
        text.push_str(
            "A longer paragraph that wraps across the width of the view, again and again. ",
        );
    }
    text.push_str("\n\nThe end.");
    text
}

fn app_over(doc: Rc<Cell<Option<Signal<String>>>>, links: Rc<RefCell<Vec<String>>>) -> App {
    App::new(move || {
        let text = signal(String::new());
        doc.set(Some(text));
        let links = links.clone();
        scroll(markdown(text).on_link(move |url, _| links.borrow_mut().push(url.to_string())))
            .follow_end()
    })
}

fn screens(fragment: usize) -> (Vec<String>, Vec<String>) {
    let text = answer();
    let whole = {
        let doc = Rc::new(Cell::new(None));
        let mut driver = app_over(doc.clone(), Rc::default()).driver(40, 30);
        driver.settle();
        doc.get().unwrap().set(text.clone());
        driver.settle();
        driver.screen().plain()
    };
    let streamed = {
        let doc = Rc::new(Cell::new(None));
        let mut driver = app_over(doc.clone(), Rc::default()).driver(40, 30);
        driver.settle();
        let doc = doc.get().unwrap();
        let chars: Vec<char> = text.chars().collect();
        for piece in chars.chunks(fragment) {
            let piece: String = piece.iter().collect();
            doc.update(|d| d.push_str(&piece));
            driver.settle();
        }
        driver.screen().plain()
    };
    (whole, streamed)
}

#[test]
fn streamed_in_one_byte_fragments_it_ends_as_set_at_once() {
    let (whole, streamed) = screens(1);
    assert_eq!(whole, streamed);
    let screen = whole.join("\n");
    assert!(screen.contains("The end."), "{screen}");
}

#[test]
fn streamed_in_words_it_ends_as_set_at_once() {
    let (whole, streamed) = screens(7);
    assert_eq!(whole, streamed);
}

#[test]
fn an_append_damages_only_the_last_block() {
    let doc = Rc::new(Cell::new(None));
    let out = doc.clone();
    let mut driver = App::new(move || {
        let text = signal(String::new());
        out.set(Some(text));
        markdown(text)
    })
    .driver(40, 20);
    driver.settle();
    let doc = doc.get().unwrap();
    doc.set("# Title\n\nA first paragraph.\n\nThe last one".into());
    driver.settle();
    let rows = driver.screen().plain();
    let last = rows
        .iter()
        .position(|r| r.starts_with("The last one"))
        .unwrap();
    let seen = driver.stats().frames;
    doc.update(|d| d.push_str(" grows"));
    driver.settle();
    assert!(driver.screen().plain()[last].starts_with("The last one grows"));
    let damage = driver.damage_since(seen).unwrap();
    assert!(!damage.is_empty());
    assert!(
        damage.iter().all(|r| r.y as usize >= last),
        "{damage:?} above row {last}"
    );
}

#[test]
fn an_open_fence_draws_as_code_and_does_not_jump_when_it_closes() {
    let doc = Rc::new(Cell::new(None));
    let mut driver = app_over(doc.clone(), Rc::default()).driver(30, 10);
    let doc = doc.get().unwrap();
    doc.set("Code:\n\n```\nlet x = 1;\nlet y".into());
    driver.settle();
    let open = driver.screen().plain();
    let at = open.iter().position(|r| r.contains("let x = 1;")).unwrap();
    // As code: no Markdown read in it.
    doc.update(|d| d.push_str(" = **2**;"));
    driver.settle();
    assert!(driver.screen().plain()[at + 1].contains("let y = **2**;"));
    let before = driver.screen().plain();
    doc.update(|d| d.push_str("\n```"));
    driver.settle();
    assert_eq!(driver.screen().plain(), before);
}

#[test]
fn tab_reaches_a_link_and_enter_or_a_click_follows_it() {
    let doc = Rc::new(Cell::new(None));
    let links = Rc::new(RefCell::new(Vec::new()));
    let mut driver = App::new({
        let (doc, links) = (doc.clone(), links.clone());
        move || {
            let text = signal(
                "No links here.\n\nSee [one](https://one.example) and [two](https://two.example)."
                    .to_string(),
            );
            doc.set(Some(text));
            let links = links.clone();
            column([
                button("before", |_| {}),
                markdown(text).on_link(move |url, _| links.borrow_mut().push(url.to_string())),
            ])
        }
    })
    .driver(40, 6);
    driver.settle();
    // The button, then the block with links: its first link.
    driver.press("tab enter");
    assert_eq!(*links.borrow(), ["https://one.example"]);
    // Tab goes to its next link.
    driver.press("tab enter");
    assert_eq!(links.borrow().last().unwrap(), "https://two.example");
    // A click on a link follows it.
    let rows = driver.screen().plain();
    let row = rows.iter().position(|r| r.starts_with("See one")).unwrap();
    driver.click(4, row as u16);
    assert_eq!(links.borrow().last().unwrap(), "https://one.example");
    assert_eq!(links.borrow().len(), 3);
}

#[test]
fn a_stream_batches_fragments_from_another_thread() {
    let doc = Rc::new(Cell::new(None));
    let app = App::new({
        let doc = doc.clone();
        move || {
            let text = signal(String::new());
            doc.set(Some(text));
            let stream = MarkdownStream::new(text);
            let sender = stream.clone();
            std::thread::spawn(move || {
                for word in answer().split_inclusive(' ') {
                    sender.append(word);
                }
            })
            .join()
            .unwrap();
            stream.finish();
            markdown(text)
        }
    });
    let mut driver = app.driver(40, 30);
    driver.settle();
    assert_eq!(doc.get().unwrap().get_untracked(), answer());
}

#[test]
fn a_whole_replacement_splits_from_the_start() {
    // The old document begins with blank lines, so its first block does
    // not start at 0; replacing the document must not start from there.
    let doc = Rc::new(Cell::new(None));
    let mut driver = app_over(doc.clone(), Rc::default()).driver(40, 10);
    driver.settle();
    let text = doc.get().unwrap();
    text.set("\n\nold".into());
    driver.settle();
    assert!(driver.screen().plain().iter().any(|l| l.contains("old")));
    text.set(String::new());
    driver.settle();
    assert!(driver.screen().plain().iter().all(|l| l.trim().is_empty()));
    text.set("Brand new text".into());
    driver.settle();
    assert!(
        driver
            .screen()
            .plain()
            .iter()
            .any(|l| l.contains("Brand new text")),
        "{:?}",
        driver.screen().plain()
    );
}
