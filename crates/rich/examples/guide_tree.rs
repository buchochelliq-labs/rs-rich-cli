//! Guide: Tree — run: cargo run -p rs-rich --example guide_tree [-- --svg docs/media/guide]
//!
//! The snippets in docs/guide/core/tree.md are cut from this file.

#[path = "guide_support/mod.rs"]
mod guide_support;

use guide_support::Shots;

use rich::{Console, Constrain, Panel, Tree};

fn main() {
    let shots = Shots::from_args("guide_tree");
    shots.shot("basic", 50, basic);
    shots.shot("build", 50, build_from_data);
    shots.shot("wrap", 40, wrapping);
    shots.shot("panel", 50, in_a_panel);
}

// --8<-- [start:basic]
fn basic(console: &Console) {
    let mut tree = Tree::new("rs-rich-cli");

    // `add` returns the new child, so bind it to keep descending.
    let crates = tree.add("crates");
    crates.add("rich");
    let ext = crates.add("rich-ext");
    ext.add("src");
    ext.add("examples");
    crates.add("rich-cli");

    tree.add("docs").add("guide").add("core");
    tree.add("Cargo.toml");

    console.print(&tree);
}
// --8<-- [end:basic]

// --8<-- [start:build]
/// Anything hierarchical maps onto `add` with a little recursion.
enum Node {
    Dir(&'static str, Vec<Node>),
    File(&'static str),
}

fn attach(parent: &mut Tree, node: &Node) {
    match node {
        Node::File(name) => {
            parent.add(*name);
        }
        Node::Dir(name, children) => {
            let branch = parent.add(format!("{name}/"));
            for child in children {
                attach(branch, child);
            }
        }
    }
}

fn build_from_data(console: &Console) {
    let src = Node::Dir(
        "src",
        vec![
            Node::File("lib.rs"),
            Node::Dir("render", vec![Node::File("mod.rs"), Node::File("table.rs")]),
            Node::File("main.rs"),
        ],
    );
    let mut tree = Tree::new("project/");
    attach(&mut tree, &src);
    attach(&mut tree, &Node::File("README.md"));
    console.print(&tree);
}
// --8<-- [end:build]

// --8<-- [start:wrap]
fn wrapping(console: &Console) {
    let mut tree = Tree::new("Release checklist");
    let checks = tree.add("Before tagging");
    checks.add("Run the full test suite on every supported platform, including the MSRV");
    checks.add("Regenerate golden fixtures");
    tree.add("After tagging: publish crates in dependency order, core first");
    console.print(&tree);
}
// --8<-- [end:wrap]

// --8<-- [start:panel]
fn in_a_panel(console: &Console) {
    let mut tree = Tree::new("services");
    let api = tree.add("api");
    api.add("v1");
    api.add("v2");
    tree.add("worker");

    let panel = Panel::new(Box::new(tree)).title("Deployment");
    console.print(&Constrain::new(Box::new(panel), Some(30)));
}
// --8<-- [end:panel]
