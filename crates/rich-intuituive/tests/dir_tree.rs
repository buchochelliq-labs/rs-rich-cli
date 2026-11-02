//! `files::dir_tree` (#814): a directory tree whose folders load as they
//! open.
#![cfg(feature = "files")]

use std::cell::RefCell;
use std::rc::Rc;

use intuituive::files::dir_tree;
use intuituive::prelude::*;

fn hidden(path: &std::path::Path) -> bool {
    path.file_name()
        .is_some_and(|name| name.to_string_lossy().starts_with('.'))
}

#[test]
fn folders_load_off_thread_the_filter_hides_dotfiles_and_files_open() {
    let dir = std::env::temp_dir().join(format!("intuituive-dir-tree-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(dir.join("src")).unwrap();
    std::fs::write(dir.join("src/main.rs"), "").unwrap();
    std::fs::write(dir.join(".env"), "").unwrap();
    std::fs::write(dir.join("README.md"), "").unwrap();
    let opened = Rc::new(RefCell::new(Vec::new()));
    let (root, seen) = (dir.clone(), opened.clone());
    let app = App::new(move || {
        let opened = opened.clone();
        dir_tree(root.clone())
            .filter(|path| !hidden(path))
            .on_open(move |path, _| opened.borrow_mut().push(path))
            .view()
    });
    // Not waiting for tasks: the folder shows `loading…` until the
    // worker's result arrives.
    let mut driver = app.driver(40, 6);
    driver.press("right");
    let mut shown = driver.screen().plain().join("\n");
    for _ in 0..200 {
        if shown.contains("src/") {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(10));
        driver.advance(std::time::Duration::from_millis(10));
        shown = driver.screen().plain().join("\n");
    }
    assert!(shown.contains("src/"), "{shown}");
    assert!(shown.contains("README.md"), "{shown}");
    assert!(!shown.contains(".env"), "{shown}");
    // Folders first.
    assert!(shown.find("src/") < shown.find("README.md"), "{shown}");
    driver.press("down down enter");
    assert_eq!(*seen.borrow(), vec![dir.join("README.md")]);
    let _ = std::fs::remove_dir_all(&dir);
}
