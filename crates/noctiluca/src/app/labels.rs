//! Source policy composed from shared measured controls, scrolling and modal scopes.
use super::*;
use std::collections::HashSet;

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
struct Spec {
    left: u16,
    top: u16,
    right: u16,
    bottom: u16,
    width: u16,
    dialog: bool,
    modal: bool,
    name: String,
    background: Option<String>,
    foreground: Vec<NodeId>,
    skip: u64,
}
fn spec(b: Browser, document: u64, node: NodeId) -> Option<Spec> {
    let (columns, height) = b.viewport.get();
    let rows = height.saturating_sub(CHROME_ROWS).max(1);
    let source_y = source_position(b);
    b.page.with(|page| {
        let page = page.as_ref().filter(|p| p.document == document)?;
        let dialog = page.overlays.iter().find(|d| d.node == node)?;
        let layer = page.layers.get(dialog.layer)?;
        let width =
            ((layer.rect.width / f64::from(CELL.0)).ceil().max(1.0) as u16).min(columns.max(1));
        let mut height = ((layer.rect.height / f64::from(CELL.1)).ceil().max(1.0) as u16).min(rows);
        let left = ((layer.rect.x / f64::from(CELL.0)).round().max(0.0) as u16)
            .min(columns.saturating_sub(width));
        let desired = if layer.place == crate::model::Place::Docked && page.view_height > 0.0 {
            rows.saturating_sub(
                ((page.view_height - layer.top).max(0.0) / f64::from(CELL.1)).round() as u16,
            )
        } else {
            (layer.top / f64::from(CELL.1)).round().max(0.0) as u16
        };
        let (top, skip) = if layer.fixed {
            (desired.min(rows.saturating_sub(height)), 0)
        } else {
            let (top, h, skip) = document_surface(layer, source_y, rows)?;
            height = h;
            (top, skip)
        };
        Some(Spec {
            left,
            top,
            right: columns.saturating_sub(left + width),
            bottom: rows.saturating_sub(top + height),
            width,
            dialog: dialog.dialog,
            modal: dialog.modal,
            name: dialog.name.clone(),
            background: dialog.background.clone(),
            foreground: dialog.foreground.clone(),
            skip,
        })
    })
}
fn owner_alive(b: Browser, document: u64, node: NodeId) -> bool {
    b.page.with(|p| {
        p.as_ref()
            .is_some_and(|p| p.document == document && p.overlays.iter().any(|d| d.node == node))
    })
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
enum ItemKey {
    Paint(u64),
    Control(NodeId),
}
#[derive(Clone, PartialEq)]
struct Items {
    paint: Projection,
    items: Vec<(ItemKey, usize)>,
    groups: HashMap<NodeId, Vec<NodeId>>,
    ordinary: bool,
}
fn items(page: &Page, projection: &Projection) -> Items {
    // An existing action annotation on image cells remains a source-coordinate
    // hotspot. Do not turn the image into a second button or infer ownership.
    let media_controls: HashSet<_> = projection
        .lines
        .iter()
        .enumerate()
        .filter(|(row, _)| {
            projection
                .regions
                .iter()
                .any(|r| r.row <= *row && *row < r.row + r.rows)
        })
        .flat_map(|(_, l)| l.spans.iter().filter_map(|s| s.control))
        .collect();
    let adopted: HashSet<_> = projection
        .lines
        .iter()
        .flat_map(|l| &l.spans)
        .filter_map(|s| s.control)
        .filter(|i| !media_controls.contains(i))
        .collect();
    let mut paint = projection.clone();
    let mut result = Vec::new();
    let mut seen = HashSet::new();
    let mut groups = HashMap::new();
    let mut bound = 0usize;
    for (row, line) in paint.lines.iter_mut().enumerate() {
        let controls: Vec<_> = line
            .spans
            .iter()
            .filter_map(|s| s.control)
            .filter(|i| adopted.contains(i))
            .collect();
        let mut group = Vec::new();
        for i in controls {
            if seen.insert(i)
                && let Some(control) = page.controls.get(i)
            {
                group.push(control.node);
                // A conservative height bound, not a copy of button measurement.
                bound = bound.saturating_add(control.name.len().saturating_add(16));
            }
        }
        let removed = line
            .spans
            .iter()
            .any(|s| s.control.is_some_and(|i| adopted.contains(&i)));
        for span in &mut line.spans {
            if span.control.is_some_and(|i| adopted.contains(&i)) {
                span.text = " ".repeat(rich::cells::cell_len(&span.text));
                span.control = None;
                span.style.clear();
                span.picture = None;
            }
        }
        if !removed
            || !line.plain().trim().is_empty()
            || projection
                .regions
                .iter()
                .any(|r| r.row <= row && row < r.row + r.rows)
        {
            result.push((
                ItemKey::Paint(projection.keys.get(row).copied().unwrap_or(row as u64)),
                row,
            ));
            bound = bound.saturating_add(1);
        }
        if let Some(&first) = group.first() {
            result.push((ItemKey::Control(first), row));
            groups.insert(first, group);
        }
    }
    Items {
        paint,
        items: result,
        groups,
        ordinary: bound <= 3900,
    }
}
fn action(
    b: Browser,
    document: u64,
    owner: NodeId,
    node: NodeId,
    wrap: bool,
    focused: Signal<Option<NodeId>>,
) -> Node {
    let control = b
        .page
        .with_untracked(|p| p.as_ref()?.control(node).cloned());
    let Some(control) = control else {
        return label("");
    };
    if !matches!(
        control.role,
        Role::Button | Role::Tab | Role::MenuItem | Role::Link
    ) {
        return web_widget(b, vec![node]).disabled_when(move || !owner_alive(b, document, owner));
    }
    let binding = Binding {
        document,
        node,
        revision: control.revision,
    };
    let label = move || {
        b.page.with(|p| {
            let Some(page) = p.as_ref().filter(|p| p.document == document) else {
                return String::new();
            };
            let Some(control) = page.control(node) else {
                return String::new();
            };
            let index = page.controls.iter().position(|c| c.node == node);
            let hint = index
                .and_then(|i| b.hint_marks().hints.get(&i).cloned())
                .map(|h| format!("[{h}] "))
                .unwrap_or_default();
            let name = if wrap {
                control.name.clone()
            } else {
                layout::face_in(control, b.effective_adapt(), 80)
            };
            markup(&format!("{hint}{name}"))
        })
    };
    let mut button = button_of(label, move |cx| {
        if !owner_alive(b, document, owner)
            || !b
                .page
                .with_untracked(|p| p.as_ref().is_some_and(|p| p.check(&binding).is_ok()))
        {
            return;
        }
        if let Some(index) = b
            .page
            .with_untracked(|p| p.as_ref()?.controls.iter().position(|c| c.node == node))
        {
            click_control(b, index, cx);
        }
    })
    .wrap_label(wrap)
    .name(&format!("popup-control-{node}"))
    .tooltip(markup(&control.name))
    .disabled_when(move || {
        !owner_alive(b, document, owner)
            || b.page.with(|p| {
                p.as_ref()
                    .and_then(|p| p.control(node))
                    .is_none_or(|c| c.disabled)
            })
    })
    .on_focus(move |_| focused.set(Some(node)))
    .auto();
    if focused.get_untracked() == Some(node) {
        button = button.autofocus();
    }
    button
}
fn paint_row(
    b: Browser,
    plan: Memo<Items>,
    key: ItemKey,
    cells: Rc<pictures::Cells>,
    fitted: Memo<Option<Spec>>,
) -> Node {
    let row_of = move || plan.with(|p| p.items.iter().find(|(k, _)| *k == key).map(|(_, r)| *r));
    let drawn = cells.clone();
    let line = leaf(move |console, width, _| {
        plan.with(|p| {
            row_of()
                .map(|row| {
                    vec![draw_row(
                        b,
                        console,
                        &drawn,
                        &p.paint,
                        row,
                        usize::from(width.max(1)),
                        &b.marks(),
                    )]
                })
                .unwrap_or_default()
        })
    })
    .on_click_at(move |cx, x, _| {
        if let Some(index) = plan.with_untracked(|p| {
            row_of()
                .and_then(|r| p.paint.lines.get(r))
                .and_then(|l| layout::control_at(l, usize::from(x)))
        }) {
            click_control(b, index, cx);
        }
        true
    });
    let targets = keyed(
        move || {
            plan.with(|p| {
                let Some(row) = row_of() else {
                    return Vec::new();
                };
                let Some(line) = p.paint.lines.get(row) else {
                    return Vec::new();
                };
                let mut zones = Vec::new();
                let mut x = 0;
                while x < line.width() {
                    let Some(index) = layout::control_at(line, x) else {
                        x += 1;
                        continue;
                    };
                    let start = x;
                    while x < line.width() && layout::control_at(line, x) == Some(index) {
                        x += 1;
                    }
                    let first = !p.paint.lines[..row]
                        .iter()
                        .any(|l| l.spans.iter().any(|s| s.control == Some(index)));
                    zones.push((
                        index,
                        start as u16,
                        x as u16,
                        first,
                        fitted.with(|s| s.as_ref().map_or(1, |s| s.width)),
                    ));
                }
                zones
            })
        },
        move |zones| {
            layers(zones.into_iter().map(|(index, start, end, first, width)| {
                let control = b
                    .page
                    .with_untracked(|p| p.as_ref()?.controls.get(index).cloned());
                let Some(c) = control else { return label("") };
                let target = c.node;
                let cells = cells.clone();
                let mut zone = leaf(move |console, _, _| {
                    plan.with(|p| {
                        row_of()
                            .map(|row| {
                                vec![segment_columns(
                                    draw_row(
                                        b,
                                        console,
                                        &cells,
                                        &p.paint,
                                        row,
                                        p.paint.lines[row].width(),
                                        &b.marks(),
                                    ),
                                    usize::from(start),
                                    usize::from(end - start),
                                )]
                            })
                            .unwrap_or_default()
                    })
                })
                .label(&c.name)
                .name(&format!("popup-control-{target}"))
                .tooltip(markup(&c.name))
                .disabled_when(move || c.disabled)
                .on_key("enter", move |cx| click_control(b, index, cx))
                .on_click(move |cx| click_control(b, index, cx))
                .inset(0, width.saturating_sub(end), 0, start);
                if first {
                    zone = zone.focusable().focus_style("reverse");
                } else {
                    zone = zone.no_focus();
                }
                zone
            }))
            .children_cover_only()
        },
    )
    .children_cover_only();
    layers([line, targets]).fixed(1)
}
fn popup(b: Browser, document: u64, owner: NodeId) -> Node {
    let fitted = memo(move || spec(b, document, owner));
    let projection = memo(move || {
        let width = fitted.with(|s| s.as_ref().map_or(1, |s| s.width));
        b.page.with(|p| {
            let Some(page) = p.as_ref().filter(|p| p.document == document) else {
                return Projection::default();
            };
            let Some(index) = page.overlays.iter().position(|d| d.node == owner) else {
                return Projection::default();
            };
            b.source
                .with(
                    |source| match source.as_ref().filter(|s| Arc::ptr_eq(s.page(), page)) {
                        Some(source) => layout::project_source_overlay(
                            source,
                            index,
                            width,
                            (CELL.0 as f64, CELL.1 as f64),
                            &b.hint_marks(),
                        ),
                        None => layout::project_overlay(
                            page,
                            index,
                            width,
                            (CELL.0 as f64, CELL.1 as f64),
                            &b.hint_marks(),
                        ),
                    },
                )
                .unwrap_or_default()
        })
    });
    let plan = memo(move || {
        b.page.with(|p| {
            p.as_ref()
                .map(|page| items(page, &projection.get()))
                .unwrap_or_else(|| Items {
                    paint: Projection::default(),
                    items: Vec::new(),
                    groups: HashMap::new(),
                    ordinary: true,
                })
        })
    });
    let focused = signal(None);
    let cells = Rc::new(pictures::Cells::with_presentation(b.presentation));
    let build = Rc::new(move |key| match key {
        ItemKey::Paint(_) => paint_row(b, plan, key, cells.clone(), fitted),
        ItemKey::Control(first) => keyed(
            move || plan.with(|p| p.groups.get(&first).cloned().unwrap_or_default()),
            move |nodes| {
                let max = nodes.len().max(1);
                grid(
                    [],
                    nodes.into_iter().map(move |node| {
                        keyed(
                            move || {
                                (
                                    b.effective_adapt().on(crate::adapt::Rule::WrapActionLabels),
                                    b.page.with(|p| {
                                        p.as_ref().and_then(|p| p.control(node)).map(|c| c.revision)
                                    }),
                                )
                            },
                            move |(wrap, _)| action(b, document, owner, node, wrap, focused),
                        )
                        .auto()
                    }),
                )
                .adaptive_columns(18, max)
                .rows([Size::Auto])
                .gap(1)
                .auto()
            },
        )
        .auto(),
    });
    let offset = signal(fitted.with_untracked(|s| s.as_ref().map_or(0, |s| s.skip)));
    let ordinary_offset = signal(offset.get_untracked().min(u16::MAX as u64) as u16);
    let rows = keyed(
        move || plan.with(|p| p.ordinary),
        move |ordinary| {
            if ordinary {
                let build = build.clone();
                scroll_with(
                    each(
                        move || plan.with(|p| p.items.iter().map(|(k, _)| *k).collect()),
                        move |key| build(key),
                    )
                    .auto(),
                    ordinary_offset,
                )
                .no_focus()
            } else {
                let build = build.clone();
                scroll_virtual_with(
                    move || plan.with(|p| p.items.len()),
                    |_| 1,
                    move |i| match plan.with_untracked(|p| p.items.get(i).map(|(k, _)| *k)) {
                        Some(key) => build(key),
                        None => label(""),
                    },
                    offset,
                )
                .item_key(move |i| {
                    plan.with_untracked(|p| {
                        p.items.get(i).map_or(i as u64, |(key, _)| match key {
                            ItemKey::Paint(k) => k.wrapping_mul(2),
                            ItemKey::Control(n) => (*n as u64).wrapping_mul(2).wrapping_add(1),
                        })
                    })
                })
            }
        },
    );
    let background = leaf(move |_, width, height| {
        let style = fitted.with(|s| {
            s.as_ref()
                .and_then(|s| s.background.as_ref())
                .map(|c| popup_fill(b.effective_adapt(), c, crate::adapt::background()))
                .unwrap_or_default()
        });
        vec![
            vec![rich::Segment::new(
                " ".repeat(width as usize),
                Some(rich::Style::parse(&style).unwrap_or_default())
            )];
            height as usize
        ]
    })
    .on_click(|_| {})
    .no_focus();
    let initial = fitted.get_untracked();
    let mut content =
        layers([background, rows]).label(initial.as_ref().map_or("", |s| s.name.as_str()));
    if initial.as_ref().is_some_and(|s| s.dialog) {
        content = content.role(rich_intuituive::a11y::Role::Dialog);
    }
    if initial.as_ref().is_some_and(|s| s.modal) {
        content = content.controlled_modal();
    }
    // Live spacer sizes preserve the measured scroll and control instances on resize.
    let gap = |edge: usize| {
        column([]).children_cover_only().size_with(move || {
            Size::Fixed(fitted.with(|s| {
                s.as_ref()
                    .map_or(0, |s| [s.top, s.right, s.bottom, s.left][edge])
            }))
        })
    };
    let positioned = column([
        gap(0),
        row([gap(3), content.flex(1), gap(1)])
            .flex(1)
            .children_cover_only(),
        gap(2),
    ])
    .children_cover_only();
    let foreground = keyed(
        move || fitted.get(),
        move |s| match s {
            Some(s) => source_foreground(b, s.foreground, (s.left, s.top, s.right, s.bottom)),
            None => label(""),
        },
    )
    .children_cover_only();
    layers([positioned, foreground]).children_cover_only()
}
pub(super) fn source_dialogs(b: Browser) -> Node {
    keyed(
        move || {
            b.page.with(|p| {
                p.as_ref()
                    .map(|p| {
                        p.overlays
                            .iter()
                            .filter(|d| spec(b, p.document, d.node).is_some())
                            .map(|d| (p.document, d.node, d.modal, d.dialog, d.name.clone()))
                            .collect::<Vec<_>>()
                    })
                    .unwrap_or_default()
            })
        },
        move |owners| {
            layers(
                owners
                    .into_iter()
                    .map(move |(document, node, _, _, _)| popup(b, document, node)),
            )
            .children_cover_only()
        },
    )
    .children_cover_only()
}
pub(super) fn open(b: Browser) {
    if let Some(owner) = b.page.with_untracked(|p| {
        p.as_ref()
            .map(|p| (p.document, p.overlays.last().map(|d| d.node)))
    }) {
        b.label_details.set(Some(owner));
    }
}
pub(super) fn details_owner_alive(b: Browser) -> bool {
    b.label_details.with(|owner| match owner {
        None => true,
        Some((document, owner)) => b.page.with(|p| {
            p.as_ref().is_some_and(|p| {
                p.document == *document
                    && owner.is_none_or(|node| p.overlays.iter().any(|d| d.node == node))
            })
        }),
    })
}
pub(super) fn details(b: Browser) -> Node {
    keyed(
        move || b.label_details.get(),
        move |owner| {
            let Some((document, owner)) = owner else {
                return column([]).children_cover_only();
            };
            let body = text(move || {
                b.page.with(|p| {
                    let Some(page) = p.as_ref().filter(|p| p.document == document) else {
                        return String::new();
                    };
                    let allowed = owner
                        .and_then(|node| page.overlays.iter().position(|d| d.node == node))
                        .and_then(|index| {
                            layout::project_overlay(
                                page,
                                index,
                                b.viewport.get().0,
                                (CELL.0 as f64, CELL.1 as f64),
                                &b.hint_marks(),
                            )
                        })
                        .map(|p| {
                            p.lines
                                .iter()
                                .flat_map(|l| &l.spans)
                                .filter_map(|s| s.control)
                                .collect::<HashSet<_>>()
                        });
                    page.controls
                        .iter()
                        .enumerate()
                        .filter(|(i, _)| allowed.as_ref().is_none_or(|a| a.contains(i)))
                        .map(|(_, c)| {
                            format!(
                                "{}{}",
                                if c.disabled { "[dim](disabled) " } else { "" },
                                format_args!(
                                    "{}{}",
                                    markup(&c.name),
                                    if c.disabled { "[/]" } else { "" }
                                )
                            )
                        })
                        .collect::<Vec<_>>()
                        .join("\n\n")
                })
            });
            layers([
                fill(" ", "black on black"),
                scroll_with(body, signal(0))
                    .autofocus()
                    .name("action-label-details")
                    .panel("Action labels · Esc closes")
                    .inset(1, 1, 1, 1),
            ])
            .controlled_modal()
        },
    )
    .no_focus()
    .children_cover_only()
}
