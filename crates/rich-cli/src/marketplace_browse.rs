//! `rich plugin browse` (#463): the marketplace in rs-rich-interact's
//! selector. Each package's preview shows its provenance (tier, source,
//! revision, artifact hash, permissions and what is installed); Enter opens
//! the operations that apply to it (install, update, remove, enable,
//! disable), which run as their commands do, prompt included, and return
//! to the list. Escape leaves.
use super::*;

use rich_interact::{Fallback, Item, Outcome as Ran, Preview, PreviewLayout, Select};

pub(super) fn run(options: &Options) -> Outcome<Done> {
    let run_options = crate::interactive::run_options(Fallback::Error, options.no_color);
    if run_options
        .policy
        .detect_for(run_options.session.output)
        .is_err()
    {
        return Err(input(
            "`rich plugin browse` needs a terminal; `rich plugin search` and `rich plugin info` \
             work without one",
        ));
    }
    let mut focus = 0;
    loop {
        let (_dir, market, lock, state) = open_for_browse(options)?;
        let listings = market.listings()?;
        if listings.is_empty() {
            return Ok(Done::new(
                "The marketplace lists nothing.\n".into(),
                json!({}),
            ));
        }
        let items: Vec<Item<String>> = listings
            .iter()
            .map(|listing| item(listing, lock.get(&listing.id), &state))
            .collect();
        let mut select = Select::new("Plugins", items)
            .preview(PreviewLayout::Auto)
            .height(12)
            .default(focus.min(listings.len() - 1));
        let id = match rich_interact::run(&mut select, &run_options) {
            Ok(Ran::Done(id)) => id,
            Ok(_) => return Ok(Done::new(String::new(), json!({}))),
            Err(error) => return Err(input(error.to_string())),
        };
        focus = listings.iter().position(|l| l.id == id).unwrap_or(0);
        let listing = &listings[focus];
        let installed = lock.get(&id);
        let operations = operations(listing, installed, &state);
        let mut menu = Select::new(
            format!("{} ({})", line(&listing.id), listing.kind),
            operations
                .iter()
                .map(|(command, label)| Item::new(command.to_string(), *label))
                .collect::<Vec<_>>(),
        )
        .preview(PreviewLayout::Hidden);
        let command = match rich_interact::run(&mut menu, &run_options) {
            Ok(Ran::Done(command)) => command,
            Ok(Ran::Interrupted) => return Ok(Done::new(String::new(), json!({}))),
            Ok(_) => continue,
            Err(error) => return Err(input(error.to_string())),
        };
        if command == "back" {
            continue;
        }
        match run_on(options, &command, &id) {
            Ok(done) if done.declined => eprintln!("Cancelled."),
            Ok(done) => eprint!("{}", done.text),
            Err(failure) => eprintln!("rich: {}", controls::shown(&failure.message)),
        }
    }
}

/// The operations that apply to a package, and `back`.
fn operations(
    listing: &Listing,
    installed: Option<&LockEntry>,
    state: &State,
) -> Vec<(&'static str, &'static str)> {
    let mut out = Vec::new();
    match installed {
        None if listing.kind.is_installable() => out.push(("install", "Install")),
        None => {}
        Some(entry) => {
            out.push(("update", "Update to the newest version"));
            if state.is_enabled(&entry.id) {
                out.push(("disable", "Disable (keep it installed, do not load it)"));
            } else {
                out.push(("enable", "Enable"));
            }
            out.push(("remove", "Remove"));
        }
    }
    out.push(("back", "Back to the list"));
    out
}

fn item(listing: &Listing, installed: Option<&LockEntry>, state: &State) -> Item<String> {
    let host = Host::current();
    let candidate = resolve(listing, &host, &Request::Newest).ok();
    let version = candidate.or_else(|| listing.versions.first());
    let mut label = format!("{}  {}", line(&listing.id), listing.tier);
    if let Some(entry) = installed {
        label.push_str(&format!("  installed {}", entry.version));
        if !state.is_enabled(&entry.id) {
            label.push_str(" (disabled)");
        }
    }
    let mut item = Item::new(listing.id.clone(), label)
        .description(line(&listing.description))
        .keyword(line(&listing.name))
        .keyword(listing.kind.as_str());
    for keyword in &listing.keywords {
        item = item.keyword(line(keyword));
    }
    let mut markup = format!(
        "[bold]{}[/] {}\n\n",
        escape(&listing.name),
        escape(&listing.description)
    );
    if let Some(version) = version {
        for (key, value) in provenance(listing, version) {
            markup.push_str(&format!("[bold]{key}[/]  {}\n", escape(&value)));
        }
        if candidate.is_none() {
            markup.push_str("\n[yellow]No version of it suits this host.[/]\n");
        }
        if listing.kind == Kind::Crate {
            markup.push_str(&format!(
                "\nA Cargo crate, listed only; add it with\n  {}\n",
                escape(&cargo_add(listing, version))
            ));
        }
    }
    if let Some(entry) = installed {
        markup.push_str(&format!(
            "\n[bold]Installed[/]  {} from index serial {}, SHA-256 {}\n",
            escape(&entry.version),
            entry.index_serial,
            escape(&entry.sha256)
        ));
        if let Some(reason) = state.yank_reason(entry) {
            markup.push_str(&format!("[red]Yanked:[/] {}\n", escape(reason)));
        }
    }
    item.preview(Preview::Markup(markup))
}
