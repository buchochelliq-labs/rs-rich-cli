//! The transforms `docutils.core.publish_doctree` applies, in docutils'
//! order, for what `rich-rst` 2.x shows: substitutions, the document title
//! and subtitle, bibliographic fields, footnote numbers, hyperlink targets,
//! the table of contents, and transitions.

use std::collections::HashMap;

use super::doctree::{Element, Node};
use super::inline::normalize_name;

pub(super) fn apply(document: &mut Element) {
    substitutions(document);
    if promote_title(document) {
        promote_subtitle(document);
    }
    docinfo(document);
    embedded_targets(document);
    footnotes(document);
    hyperlinks(document);
    citations(document);
    contents(document);
    transitions(document);
}

/// docutils' `PreBibliographic` nodes: what may stand before the title.
fn pre_bibliographic(node: &Node) -> bool {
    match node {
        Node::Text(_) => false,
        Node::Element(element) => matches!(
            element.tag,
            "title"
                | "subtitle"
                | "meta"
                | "decoration"
                | "comment"
                | "substitution_definition"
                | "target"
                | "pending"
                | "system_message"
                | "raw"
        ),
    }
}

/// `candidate_index`: the lone section after the pre-bibliographic nodes.
fn candidate_index(node: &Element) -> Option<usize> {
    let index = node
        .children
        .iter()
        .position(|child| !pre_bibliographic(child))?;
    (index + 1 == node.children.len() && node.children[index].is("section")).then_some(index)
}

fn promote_title(document: &mut Element) -> bool {
    let Some(index) = candidate_index(document) else {
        return false;
    };
    let Node::Element(section) = document.children.remove(index) else {
        return false;
    };
    document.names.clone_from(&section.names);
    let mut children = section.children.into_iter();
    let title = children.next();
    let before: Vec<Node> = std::mem::take(&mut document.children);
    document.children = title.into_iter().chain(before).chain(children).collect();
    true
}

fn promote_subtitle(document: &mut Element) {
    let Some(index) = candidate_index(document) else {
        return;
    };
    let Node::Element(section) = document.children.remove(index) else {
        return;
    };
    let mut children = section.children.into_iter();
    let Some(Node::Element(title)) = children.next() else {
        return;
    };
    let mut subtitle = Element::new("subtitle");
    subtitle.children = title.children;
    subtitle.names = section.names;
    document.children.insert(1, subtitle.into());
    document.children.extend(children);
}

/// The fields `DocInfo` turns into their own elements.
const BIBLIOGRAPHIC: &[&str] = &[
    "author",
    "organization",
    "address",
    "contact",
    "version",
    "revision",
    "status",
    "date",
    "copyright",
];

/// `DocInfo`: a field list first in the document becomes its bibliographic
/// fields.
fn docinfo(document: &mut Element) {
    let Some(index) = document
        .children
        .iter()
        .position(|child| !pre_bibliographic(child))
    else {
        return;
    };
    if !document.children[index].is("field_list") {
        return;
    }
    let Node::Element(list) = document.children.remove(index) else {
        return;
    };
    let mut info = Element::new("docinfo");
    let mut topics = Vec::new();
    for field in list.children {
        let Node::Element(field) = field else {
            continue;
        };
        let name = field
            .find("field_name")
            .map(Element::astext)
            .unwrap_or_default();
        let key = name.to_lowercase();
        let body = field.find("field_body").cloned().unwrap_or_default();
        let lone_paragraph = match body.children.as_slice() {
            [Node::Element(paragraph)] if paragraph.tag == "paragraph" => Some(paragraph.clone()),
            _ => None,
        };
        if let (Some(tag), Some(paragraph)) = (
            BIBLIOGRAPHIC.iter().find(|tag| **tag == key),
            lone_paragraph.as_ref(),
        ) {
            let mut element = Element::new(tag);
            element.children.clone_from(&paragraph.children);
            info.children.push(element.into());
            continue;
        }
        if key == "authors" {
            if let Some(authors) = authors(&body) {
                info.children.push(authors.into());
                continue;
            }
        }
        if key == "dedication" || key == "abstract" {
            let title = if key == "dedication" {
                "Dedication"
            } else {
                "Abstract"
            };
            topics.push(
                Element::new("topic")
                    .class(key.clone())
                    .child(Element::text("title", title))
                    .children(body.children)
                    .into(),
            );
            continue;
        }
        let mut field = field;
        field.classes.push(super::parse::make_id(&name));
        info.children.push(field.into());
    }
    let at = document
        .children
        .iter()
        .position(|child| !matches!(child, Node::Element(e) if matches!(e.tag, "title" | "subtitle" | "decoration" | "meta")))
        .unwrap_or(document.children.len());
    let mut nodes: Vec<Node> = Vec::new();
    if !info.children.is_empty() {
        nodes.push(info.into());
    }
    nodes.extend(topics);
    document.children.splice(at..at, nodes);
}

/// The `authors` field: one paragraph split on `;` (or `,`), or a bullet
/// list of one paragraph each.
fn authors(body: &Element) -> Option<Element> {
    let mut out = Element::new("authors");
    match body.children.as_slice() {
        [Node::Element(paragraph)] if paragraph.tag == "paragraph" => {
            let text = paragraph.astext();
            let separator = if text.contains(';') { ';' } else { ',' };
            for name in text
                .split(separator)
                .map(str::trim)
                .filter(|n| !n.is_empty())
            {
                out.children.push(Element::text("author", name).into());
            }
        }
        [Node::Element(list)] if list.tag == "bullet_list" => {
            for item in list.elements() {
                let [Node::Element(paragraph)] = item.children.as_slice() else {
                    return None;
                };
                let mut author = Element::new("author");
                author.children.clone_from(&paragraph.children);
                out.children.push(author.into());
            }
        }
        _ => return None,
    }
    (!out.children.is_empty()).then_some(out)
}

/// Every substitution definition, by name.
fn definitions(element: &Element, found: &mut HashMap<String, Vec<Node>>) {
    for child in element.elements() {
        if child.tag == "substitution_definition" {
            if let Some(name) = child.get("name") {
                found
                    .entry(name.to_string())
                    .or_insert_with(|| child.children.clone());
                found
                    .entry(normalize_name(name))
                    .or_insert_with(|| child.children.clone());
            }
        } else {
            definitions(child, found);
        }
    }
}

/// `Substitutions`: each reference is replaced by its definition's
/// content; an unknown one is problematic, shown as its source.
fn substitutions(document: &mut Element) {
    let mut defined = HashMap::new();
    definitions(document, &mut defined);
    replace_substitutions(document, &defined);
}

fn replace_substitutions(element: &mut Element, defined: &HashMap<String, Vec<Node>>) {
    if element.tag == "substitution_definition" {
        return;
    }
    let mut out = Vec::with_capacity(element.children.len());
    for child in std::mem::take(&mut element.children) {
        let Node::Element(mut child) = child else {
            out.push(child);
            continue;
        };
        if child.tag == "substitution_reference" {
            let name = child.astext();
            match defined
                .get(&name)
                .or_else(|| defined.get(&normalize_name(&name)))
            {
                Some(content) => out.extend(content.iter().cloned()),
                None => out.push(Element::text("problematic", format!("|{name}|")).into()),
            }
            continue;
        }
        // `|name|_`: the reference around a substitution.
        if child.tag == "reference"
            && matches!(child.children.as_slice(), [Node::Element(s)] if s.tag == "substitution_reference")
        {
            let name = child.children[0].astext();
            match defined
                .get(&name)
                .or_else(|| defined.get(&normalize_name(&name)))
            {
                Some(content) => child.children = content.clone(),
                None => {
                    let end = if child.get("anonymous").is_some() {
                        "__"
                    } else {
                        "_"
                    };
                    out.push(Element::text("problematic", format!("|{name}|{end}")).into());
                    continue;
                }
            }
        }
        replace_substitutions(&mut child, defined);
        out.push(child.into());
    }
    element.children = out;
}

/// A reference with an embedded URI and a name is also a target for that
/// name, standing after it.
fn embedded_targets(element: &mut Element) {
    let mut out = Vec::with_capacity(element.children.len());
    for child in std::mem::take(&mut element.children) {
        let Node::Element(mut child) = child else {
            out.push(child);
            continue;
        };
        embedded_targets(&mut child);
        let target = child.attrs.remove("__target").map(|name| {
            let mut target = Element::new("target").with(
                "refuri",
                child.get("refuri").unwrap_or_default().to_string(),
            );
            target.names = vec![name];
            target
        });
        out.push(child.into());
        if let Some(target) = target {
            out.push(target.into());
        }
    }
    element.children = out;
}

const SYMBOLS: [&str; 10] = [
    "*", "\u{2020}", "\u{2021}", "\u{a7}", "\u{b6}", "#", "\u{2660}", "\u{2665}", "\u{2666}",
    "\u{2663}",
];

/// `Footnotes`: auto-numbered footnotes get the numbers not taken, their
/// references the same, and auto-symbol footnotes their symbols.
fn footnotes(document: &mut Element) {
    let mut taken: Vec<String> = Vec::new();
    document.walk(&mut |element| {
        taken.extend(element.names.iter().cloned());
    });
    // Number the auto-numbered footnotes, in order.
    let mut number = 1;
    let mut labels: Vec<String> = Vec::new();
    let mut named: HashMap<String, String> = HashMap::new();
    let mut symbols = 0;
    document.walk_mut(&mut |element| {
        if element.tag != "footnote" {
            return;
        }
        match element.get("auto") {
            Some("1") => {
                while taken.contains(&number.to_string()) {
                    number += 1;
                }
                let label = number.to_string();
                taken.push(label.clone());
                number += 1;
                element
                    .children
                    .insert(0, Element::text("label", label.clone()).into());
                match element.names.first() {
                    Some(name) => {
                        named.insert(name.clone(), label.clone());
                    }
                    None => {
                        labels.push(label.clone());
                        element.names = vec![label];
                    }
                }
            }
            Some("*") => {
                let symbol = SYMBOLS[symbols % SYMBOLS.len()].repeat(symbols / SYMBOLS.len() + 1);
                symbols += 1;
                element
                    .children
                    .insert(0, Element::text("label", symbol).into());
            }
            _ => {}
        }
    });
    let mut footnote_names: Vec<String> = Vec::new();
    document.walk(&mut |element| {
        if element.tag == "footnote" {
            footnote_names.extend(element.names.iter().cloned());
        }
    });
    let mut next_label = labels.into_iter();
    let mut symbol_refs = 0;
    resolve_footnote_references(document, &mut |reference| match reference.get("auto") {
        Some("1") => {
            let label = match reference.get("refname") {
                Some(name) => named.get(name).cloned(),
                None => next_label.next(),
            };
            label.map(|label| {
                reference.attrs.remove("refname");
                reference.children = vec![Node::Text(label)];
            })
        }
        Some("*") => {
            let symbol =
                SYMBOLS[symbol_refs % SYMBOLS.len()].repeat(symbol_refs / SYMBOLS.len() + 1);
            symbol_refs += 1;
            (symbol_refs <= symbols).then(|| {
                reference.children = vec![Node::Text(symbol)];
            })
        }
        _ => {
            let name = reference.get("refname").unwrap_or_default().to_string();
            footnote_names.contains(&name).then(|| {
                reference.attrs.remove("refname");
            })
        }
    });
}

/// Call `resolve` on each footnote reference; one it cannot resolve
/// (`None`) is problematic, shown as its source.
fn resolve_footnote_references(
    element: &mut Element,
    resolve: &mut dyn FnMut(&mut Element) -> Option<()>,
) {
    for child in &mut element.children {
        let Node::Element(child_element) = child else {
            continue;
        };
        if child_element.tag == "footnote_reference" {
            if resolve(child_element).is_none() {
                let source = match (child_element.get("auto"), child_element.get("refname")) {
                    (Some("*"), _) => "[*]_".to_string(),
                    (Some(_), Some(name)) => format!("[#{name}]_"),
                    (Some(_), None) => "[#]_".to_string(),
                    (None, _) => format!("[{}]_", child_element.astext()),
                };
                *child = Element::text("problematic", source).into();
            }
            continue;
        }
        resolve_footnote_references(child_element, resolve);
    }
}

/// `citation_reference`s to an unknown citation are problematic.
fn citations(document: &mut Element) {
    let mut names = Vec::new();
    document.walk(&mut |element| {
        if element.tag == "citation" {
            names.extend(element.names.iter().cloned());
        }
    });
    fn walk(element: &mut Element, names: &[String]) {
        for child in &mut element.children {
            let Node::Element(child_element) = child else {
                continue;
            };
            if child_element.tag == "citation_reference" {
                let name = child_element.get("refname").unwrap_or_default().to_string();
                if names.contains(&name) {
                    child_element.attrs.remove("refname");
                } else {
                    let text = format!("[{}]_", child_element.astext());
                    *child = Element::text("problematic", text).into();
                }
                continue;
            }
            walk(child_element, names);
        }
    }
    walk(document, &names);
}

/// Where a name leads: a URI, or somewhere in the document.
#[derive(Clone, Debug)]
enum Destination {
    Uri(String),
    Internal,
    /// Another name (an indirect target).
    Name(String),
}

/// The hyperlink transforms: anonymous references take the anonymous
/// targets in order, named references their target's URI; one with no
/// target is problematic, shown as its source.
fn hyperlinks(document: &mut Element) {
    let mut named: HashMap<String, Destination> = HashMap::new();
    let mut anonymous: Vec<Destination> = Vec::new();
    let mut anonymous_refs = 0;
    document.walk(&mut |element| match element.tag {
        "target" => {
            let destination = match (element.get("refuri"), element.get("refname")) {
                (Some(uri), _) => Destination::Uri(uri.to_string()),
                (None, Some(name)) => Destination::Name(name.to_string()),
                (None, None) => Destination::Internal,
            };
            if element.get("anonymous").is_some() {
                anonymous.push(destination);
            } else {
                for name in &element.names {
                    named.entry(name.clone()).or_insert(destination.clone());
                }
            }
        }
        "section" | "footnote" | "citation" => {
            for name in &element.names {
                named.entry(name.clone()).or_insert(Destination::Internal);
            }
        }
        "reference" if element.get("anonymous").is_some() => anonymous_refs += 1,
        _ => {}
    });
    let resolve = |name: &str| -> Option<Destination> {
        let mut name = name.to_string();
        for _ in 0..32 {
            match named.get(&name)? {
                Destination::Name(next) => name.clone_from(next),
                other => return Some(other.clone()),
            }
        }
        None
    };
    let anonymous: Vec<Option<Destination>> = anonymous
        .into_iter()
        .map(|destination| match destination {
            Destination::Name(name) => resolve(&name),
            other => Some(other),
        })
        .collect();
    let matched = anonymous.len() == anonymous_refs;
    let mut next_anonymous = anonymous.into_iter();
    fn walk(
        element: &mut Element,
        resolve: &dyn Fn(&str) -> Option<Destination>,
        anonymous: &mut dyn Iterator<Item = Option<Destination>>,
        matched: bool,
    ) {
        for child in &mut element.children {
            let Node::Element(reference) = child else {
                continue;
            };
            if reference.tag != "reference" {
                walk(reference, resolve, anonymous, matched);
                continue;
            }
            let destination = if reference.get("anonymous").is_some() {
                if matched {
                    anonymous.next().flatten()
                } else {
                    None
                }
            } else if reference.get("refuri").is_some() {
                walk(reference, resolve, anonymous, matched);
                continue;
            } else if let Some(name) = reference.get("refname") {
                resolve(name)
            } else {
                walk(reference, resolve, anonymous, matched);
                continue;
            };
            match destination {
                Some(Destination::Uri(uri)) => {
                    reference.attrs.remove("refname");
                    reference.attrs.insert("refuri", uri);
                }
                Some(_) => {
                    reference.attrs.remove("refname");
                }
                None => {
                    let raw = reference
                        .get("__raw")
                        .map_or_else(|| format!("{}_", reference.astext()), String::from);
                    *child = Element::text("problematic", raw).into();
                    continue;
                }
            }
            walk(reference, resolve, anonymous, matched);
        }
    }
    walk(document, &resolve, &mut next_anonymous, matched);
    document.walk_mut(&mut |element| {
        element.attrs.remove("__raw");
    });
}

/// `Contents`: a `contents` topic gets a bullet list of the section titles
/// after it (or, `:local:`, of the sections it stands in).
fn contents(document: &mut Element) {
    fn entries(element: &Element, depth: Option<usize>, level: usize) -> Option<Element> {
        if depth.is_some_and(|depth| level > depth) {
            return None;
        }
        let mut list = Element::new("bullet_list");
        for section in element.elements().filter(|e| e.tag == "section") {
            let Some(title) = section.find("title") else {
                continue;
            };
            let mut reference = Element::new("reference");
            reference.children = title
                .children
                .iter()
                .filter(|child| !child.is("target") && !child.is("footnote_reference"))
                .cloned()
                .collect();
            let mut item =
                Element::new("list_item").child(Element::new("paragraph").child(reference));
            if let Some(sub) = entries(section, depth, level + 1) {
                item = item.child(sub);
            }
            list = list.child(item);
        }
        (!list.children.is_empty()).then_some(list)
    }
    fn walk(element: &mut Element, top: bool) {
        let snapshot = element.clone();
        let mut empty = Vec::new();
        for (index, child) in element.children.iter_mut().enumerate() {
            let Node::Element(child) = child else {
                continue;
            };
            if child.tag == "topic" && child.attrs.remove("__contents").is_some() {
                let depth = child
                    .attrs
                    .remove("__depth")
                    .and_then(|depth| depth.parse::<usize>().ok());
                child.attrs.remove("__local");
                // The sections of what holds it: the document's, or (local)
                // its section's.
                match entries(&snapshot, depth, 1) {
                    Some(list) => child.children.push(list.into()),
                    // No sections: the topic goes.
                    None => empty.push(index),
                }
                continue;
            }
            walk(child, false);
        }
        for index in empty.into_iter().rev() {
            element.children.remove(index);
        }
        let _ = top;
    }
    walk(document, true);
}

/// `Transitions`: one ending a section moves after the section.
fn transitions(element: &mut Element) {
    for child in &mut element.children {
        if let Node::Element(child) = child {
            if child.tag == "section" {
                transitions(child);
            }
        }
    }
    let mut index = 0;
    while index < element.children.len() {
        let moved = match &mut element.children[index] {
            Node::Element(section) if section.tag == "section" => {
                // A section that ends with a transition gives it up.
                if section
                    .children
                    .last()
                    .is_some_and(|last| last.is("transition"))
                {
                    section.children.pop()
                } else {
                    None
                }
            }
            _ => None,
        };
        if let Some(transition) = moved {
            element.children.insert(index + 1, transition);
        }
        index += 1;
    }
}
