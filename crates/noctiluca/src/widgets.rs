//! Web widgets: a page's controls as stock intuiTUIve widgets.
//!
//! Each adapter wraps one stock widget (`button`, `input`, `checkbox`,
//! `radio`, `select_field`, `slider`) and binds it to a control of the
//! page model: the widget's signal follows the page, and what the user
//! does to it becomes a guarded action on the tab ([`crate::tab`]). The
//! stock widgets are called only through the small `stock_*` functions
//! at the top, so they can switch to intuiTUIve's coming controls (live
//! labels, the three-state check box, keyed options: C1 to C3 of the
//! design note) in one place each.
//!
//! A text field is a [`DraftField`]: typing edits a draft here; Enter
//! commits it with the value it was based on, and the tab refuses the
//! commit as a conflict if the page changed the value meanwhile. A change
//! the page makes under an edited draft marks the field in conflict, and
//! never overwrites what was typed. Ctrl+R takes the page's value.

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use rich_intuituive::prelude::*;
use rich_intuituive::{
    button_of, checkbox_of, radio_keyed, select_field_keyed, slider, switch_of, CheckState,
    SelectOption,
};

use crate::app::Browser;
use crate::clean::markup;
use crate::draft::{DraftField, DraftState};
use crate::engine::NodeId;
use crate::model::{Check, Control, Page, Role};
use crate::tab::{describe, Intent, Refusal};

// The stock widgets, one function each: intuiTUIve's controls with live
// labels (C1), the three-state check box (C2) and keyed options (C3).

/// A button whose label follows the page's name for it.
fn stock_button(
    label: impl Fn() -> String + 'static,
    on_press: impl FnMut(&mut Ctx) + 'static,
) -> Node {
    button_of(label, on_press)
}

/// A check box with the page's three states: off, on, mixed.
fn stock_checkbox(label: impl Fn() -> String + 'static, state: Signal<CheckState>) -> Node {
    checkbox_of(label, state)
}

fn stock_switch(label: impl Fn() -> String + 'static, on: Signal<bool>) -> Node {
    switch_of(label, on)
}

/// Radio buttons keyed by their own nodes.
fn stock_radio(
    options: impl Fn() -> Vec<SelectOption<NodeId>> + 'static,
    selected: Signal<Option<NodeId>>,
) -> Node {
    radio_keyed(options, selected)
}

/// A select keyed by its options' own nodes, each with its availability:
/// two options with one value stay apart, and a disabled one cannot be
/// chosen in any way.
fn stock_select(
    options: impl Fn() -> Vec<SelectOption<NodeId>> + 'static,
    selected: Signal<Option<NodeId>>,
) -> Node {
    select_field_keyed(options, selected)
}

fn stock_slider(value: Signal<f64>, range: std::ops::RangeInclusive<f64>, step: f64) -> Node {
    slider(value, range, step)
}

fn stock_input(text: Signal<String>) -> Node {
    input(text)
}

/// The state behind one control's widget, kept across readings of the
/// page while the control is there.
pub struct WebControl {
    pub node: NodeId,
    pub text: Signal<String>,
    pub check: Signal<CheckState>,
    pub on: Signal<bool>,
    /// The chosen option of a select, or radio of a group, by its node.
    pub key: Signal<Option<NodeId>>,
    /// A radio group's radios, by node.
    pub group: RefCell<Vec<NodeId>>,
    pub number: Signal<f64>,
    pub draft: RefCell<DraftField<String>>,
    /// The draft's state, for the line beside the field.
    pub status: Signal<String>,
    /// An action is on its way: the widget does not follow the page until
    /// it is answered.
    pub pending: Cell<bool>,
    /// The widget's node, to focus it.
    pub widget: Cell<Option<rich_intuituive::reactive::NodeId>>,
}

impl WebControl {
    pub fn new(control: &Control) -> WebControl {
        let value = control.value.clone().unwrap_or_default();
        WebControl {
            node: control.node,
            text: signal(value.clone()),
            check: signal(check_state(control.checked)),
            on: signal(control.checked == Some(Check::On)),
            key: signal(control.options.iter().find(|o| o.selected).map(|o| o.key)),
            group: RefCell::new(Vec::new()),
            number: signal(value.parse().unwrap_or(0.0)),
            draft: RefCell::new(DraftField::new(value, control.state_revision)),
            status: signal(String::new()),
            pending: Cell::new(false),
            widget: Cell::new(None),
        }
    }

    /// Follow the page's reading of the control.
    pub fn sync(&self, page: &Page, control: &Control) {
        if control.role.is_text() {
            let value = control.value.clone().unwrap_or_default();
            let mut draft = self.draft.borrow_mut();
            draft.observe(value, control.state_revision);
            if *draft.state() == DraftState::Clean && self.text.get_untracked() != *draft.draft() {
                self.text.set(draft.draft().clone());
            }
            let status = status_of(draft.state());
            drop(draft);
            self.status.set(status);
            return;
        }
        if self.pending.get() {
            return;
        }
        let state = check_state(control.checked);
        if self.check.get_untracked() != state {
            self.check.set(state);
        }
        let checked = control.checked == Some(Check::On);
        if self.on.get_untracked() != checked {
            self.on.set(checked);
        }
        let group = self.group.borrow().clone();
        let key = if group.is_empty() {
            control.options.iter().find(|o| o.selected).map(|o| o.key)
        } else {
            group.iter().copied().find(|n| {
                page.control(*n)
                    .is_some_and(|c| c.checked == Some(Check::On))
            })
        };
        if self.key.get_untracked() != key {
            self.key.set(key);
        }
        if control.role == Role::Slider {
            if let Some(n) = control.value.as_deref().and_then(|v| v.parse::<f64>().ok()) {
                if self.number.get_untracked() != n {
                    self.number.set(n);
                }
            }
        }
    }
}

/// The page's check state as the widget's.
fn check_state(checked: Option<Check>) -> CheckState {
    match checked {
        Some(Check::On) => CheckState::On,
        Some(Check::Mixed) => CheckState::Mixed,
        _ => CheckState::Off,
    }
}

/// The line beside a text field for its draft's state.
fn status_of(state: &DraftState<String>) -> String {
    match state {
        DraftState::Clean => String::new(),
        DraftState::Dirty => "[dim]● edited · Enter applies[/]".into(),
        DraftState::Pending => "[dim]… applying[/]".into(),
        DraftState::Conflict { remote } => format!(
            "[bold yellow]⚠ the page changed it to {:?} · Ctrl+R takes it[/]",
            markup(&crate::clean::truncate(remote, 30))
        ),
        DraftState::Failed(why) => format!("[red]✗ {}[/]", markup(why)),
    }
}

/// The control `node` of the page now.
fn control_of(page: &Option<std::sync::Arc<Page>>, node: NodeId) -> Option<Control> {
    page.as_ref()?.control(node).cloned()
}

/// Run `intent` on `node` now, guarded; refusals become a toast.
fn act(browser: Browser, web: &Rc<WebControl>, node: NodeId, intent: Intent, cx: &mut Ctx) {
    let page = browser.page.with_untracked(Clone::clone);
    let Some(page) = page else { return };
    let Some(control) = page.control(node) else {
        cx.toast("[red]That control is gone from the page.[/]");
        return;
    };
    web.pending.set(true);
    let web = web.clone();
    browser.act(control.binding(&page), intent, move |result, cx| {
        web.pending.set(false);
        if let Err(refusal) = result {
            cx.toast(format!("[red]{}[/]", markup(&refusal.to_string())));
        }
        browser.refresh_controls();
    });
}

/// The widget of the controls `nodes` (one, or a group of radios).
pub fn web_widget(browser: Browser, nodes: Vec<NodeId>) -> Node {
    let Some(first) = nodes.first().copied() else {
        return label("");
    };
    let page = browser.page.with_untracked(Clone::clone);
    let Some(control) = control_of(&page, first) else {
        return label("");
    };
    let web = browser.web_control(&control);
    let disabled = move || {
        browser
            .page
            .with(|p| control_of(p, first).is_none_or(|c| c.disabled))
    };
    let name = markup(&control.name);
    let node = match control.role {
        Role::Button | Role::Tab | Role::MenuItem | Role::Link => {
            let web2 = web.clone();
            let face = move || {
                let name = browser.page.with(|p| {
                    control_of(p, first)
                        .map(|c| markup(&c.name))
                        .unwrap_or_default()
                });
                if name.is_empty() {
                    "button".to_string()
                } else {
                    name
                }
            };
            let button = mark(
                &web,
                stock_button(face, move |cx| {
                    act(browser, &web2, first, Intent::Click, cx)
                }),
            )
            .disabled_when(disabled)
            .auto();
            row([button, label("").flex(1)])
        }
        Role::CheckBox | Role::Switch => {
            let label = move || {
                browser.page.with(|p| {
                    control_of(p, first)
                        .map(|c| markup(&c.name))
                        .unwrap_or_default()
                })
            };
            let web2 = web.clone();
            if control.role == Role::Switch {
                on_write(web.on, move |on, cx| {
                    let now = browser.page.with_untracked(|p| control_of(p, first));
                    if now.is_some_and(|c| (c.checked == Some(Check::On)) != on)
                        && !web2.pending.get()
                    {
                        act(browser, &web2, first, Intent::Click, cx);
                    }
                });
                mark(&web, stock_switch(label, web.on)).disabled_when(disabled)
            } else {
                on_write(web.check, move |state, cx| {
                    let now = browser.page.with_untracked(|p| control_of(p, first));
                    // A click on a mixed box ticks it, in the page as here.
                    if now.is_some_and(|c| check_state(c.checked) != state) && !web2.pending.get() {
                        act(browser, &web2, first, Intent::Click, cx);
                    }
                });
                mark(&web, stock_checkbox(label, web.check)).disabled_when(disabled)
            }
        }
        Role::Radio => {
            let group = nodes.clone();
            web.group.replace(group.clone());
            let options = {
                let group = group.clone();
                move || {
                    browser.page.with(|p| {
                        group
                            .iter()
                            .filter_map(|n| control_of(p, *n))
                            .map(|c| SelectOption {
                                key: c.node,
                                label: markup(&c.name),
                                enabled: !c.disabled,
                                reason: c.disabled.then(|| "disabled".to_string()),
                            })
                            .collect()
                    })
                }
            };
            let checked = group
                .iter()
                .copied()
                .find(|n| control_of(&page, *n).is_some_and(|c| c.checked == Some(Check::On)));
            web.key.set(checked);
            let web2 = web.clone();
            on_write(web.key, move |key, cx| {
                let Some(target) = key else { return };
                let on = browser.page.with_untracked(|p| {
                    control_of(p, target).is_some_and(|c| c.checked == Some(Check::On))
                });
                if !on && !web2.pending.get() {
                    act(browser, &web2, target, Intent::Click, cx);
                }
            });
            let label = control.group.clone().unwrap_or_default();
            mark(&web, stock_radio(options, web.key))
                .label(&label)
                .disabled_when(disabled)
        }
        Role::ComboBox | Role::ListBox => {
            let options = move || {
                browser.page.with(|p| {
                    control_of(p, first)
                        .map(|c| {
                            c.options
                                .iter()
                                .map(|o| SelectOption {
                                    key: o.key,
                                    label: markup(&o.label),
                                    enabled: !o.disabled,
                                    reason: o.disabled.then(|| "disabled on the page".to_string()),
                                })
                                .collect()
                        })
                        .unwrap_or_default()
                })
            };
            let web2 = web.clone();
            on_write(web.key, move |key, cx| {
                let Some(key) = key else { return };
                let Some(now) = browser.page.with_untracked(|p| control_of(p, first)) else {
                    return;
                };
                let Some(option) = now.options.iter().find(|o| o.key == key) else {
                    return;
                };
                if option.selected || web2.pending.get() {
                    return;
                }
                // The tab refuses a disabled option too, should one get here.
                act(
                    browser,
                    &web2,
                    first,
                    Intent::Select { option: option.key },
                    cx,
                );
            });
            row([
                label(format!("{name} ")).auto(),
                mark(&web, stock_select(options, web.key))
                    .disabled_when(disabled)
                    .flex(1),
            ])
        }
        Role::Slider => {
            let (min, max) = (control.min.unwrap_or(0.0), control.max.unwrap_or(100.0));
            let step = control.step.unwrap_or(1.0);
            let web2 = web.clone();
            on_write(web.number, move |n, cx| {
                let now = browser.page.with_untracked(|p| control_of(p, first));
                let page_value = now
                    .and_then(|c| c.value)
                    .and_then(|v| v.parse::<f64>().ok());
                if page_value != Some(n) && !web2.pending.get() {
                    act(browser, &web2, first, Intent::SetRange(n), cx);
                }
            });
            row([
                label(format!("{name} ")).auto(),
                mark(&web, stock_slider(web.number, min..=max.max(min), step))
                    .disabled_when(disabled)
                    .fixed(24),
                {
                    let number = web.number;
                    text(move || number.get().to_string()).auto()
                },
            ])
        }
        Role::TextBox | Role::SearchBox | Role::Password | Role::SpinButton => {
            let password = control.role == Role::Password;
            let web2 = web.clone();
            on_write(web.text, move |value, _| {
                let mut draft = web2.draft.borrow_mut();
                if *draft.draft() != value && draft.set(value).is_ok() {
                    let status = status_of(draft.state());
                    drop(draft);
                    web2.status.set(status);
                }
            });
            let web_submit = web.clone();
            let mut field = stock_input(web.text)
                .on_submit(move |_, cx| commit(browser, &web_submit, first, password, cx));
            if password {
                // Its value is never read back: what is typed shows masked.
                field = field.mask('•');
            }
            let web_reload = web.clone();
            let field = mark(&web, field)
                .disabled_when(disabled)
                .on_key("ctrl+r", move |_| {
                    let mut draft = web_reload.draft.borrow_mut();
                    draft.reload();
                    let value = draft.draft().clone();
                    drop(draft);
                    web_reload.text.set(value);
                    web_reload.status.set(String::new());
                })
                .flex(1);
            let status = web.status;
            row([
                label(format!("{name} ")).auto(),
                field,
                text(move || status.get()).auto(),
            ])
        }
    };
    // Link hints label widgets too: the hint of each control of the group.
    let rows = nodes.len().max(1) as u16;
    let tag = text(move || {
        browser.hints.with(|hints| {
            let Some(hints) = hints else {
                return String::new();
            };
            browser.page.with(|p| {
                let Some(page) = p else { return String::new() };
                hints
                    .labels
                    .iter()
                    .filter(|(_, i)| {
                        page.controls
                            .get(*i)
                            .is_some_and(|c| nodes.contains(&c.node))
                    })
                    .map(|(label, _)| format!("[bold black on yellow]\\[{label}][/]"))
                    .collect()
            })
        })
    })
    .auto();
    let node = row([tag, node.flex(1)]).name(&format!("web-{first}"));
    node.fixed(rows)
}

/// Keep `node`'s id as the widget to focus for its control.
fn mark(web: &WebControl, node: Node) -> Node {
    web.widget.set(Some(node.id()));
    node
}

/// Commit a text field's draft: the tab checks the page still holds the
/// value it was based on.
fn commit(browser: Browser, web: &Rc<WebControl>, node: NodeId, password: bool, cx: &mut Ctx) {
    let request = web.draft.borrow_mut().request();
    let commit = match request {
        Ok(commit) => commit,
        Err(why) => {
            cx.toast(format!("[yellow]{}[/]", markup(&why)));
            return;
        }
    };
    web.status.set(status_of(&DraftState::Pending));
    let page = browser.page.with_untracked(Clone::clone);
    let Some(page) = page else { return };
    let Some(control) = page.control(node) else {
        return;
    };
    let value = commit.value.clone();
    let intent = Intent::SetValue {
        value: value.clone(),
        // A password's value is never read, so it has no base to check.
        base: (!password).then(|| commit.base.clone()),
    };
    let web = web.clone();
    let revision = control.state_revision;
    browser.act(control.binding(&page), intent, move |result, cx| {
        let mut draft = web.draft.borrow_mut();
        match result {
            Ok(()) => draft.accept(value, revision),
            Err(Refusal::Conflict { remote }) => draft.reject("conflict", Some(remote)),
            Err(other) => {
                cx.toast(format!("[red]{}[/]", markup(&other.to_string())));
                draft.reject(other.to_string(), None)
            }
        }
        let status = status_of(draft.state());
        drop(draft);
        web.status.set(status);
    });
}

/// How a hint or a reason names a control.
pub fn name_of(control: &Control) -> String {
    describe(control)
}
