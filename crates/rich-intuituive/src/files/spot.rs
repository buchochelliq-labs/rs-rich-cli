//! What Yazi's spotter shows about a file: its details as a description
//! list.

use std::path::Path;

use rich::markup::escape;
use rich_ext::filetype;
use rich_ext::listing::{owner, readable_size, Entry};

use super::row::coloured_permissions;
use crate::facts::facts;
use crate::node::Node;

/// The details of `path` as a description list (the core's [`facts`]),
/// as Yazi's spotter shows them: its name, path, MIME type (sniffed from
/// its first bytes), size, mode, owner, times in the local time zone and,
/// for a link, where it points. Read now; for the hovered file, make one
/// in a modal when it opens.
///
/// ```
/// use intuituive::files::spot;
/// use intuituive::prelude::*;
///
/// let path = std::env::temp_dir().join(format!("intuituive-spot-doc-{}.txt", std::process::id()));
/// std::fs::write(&path, "hello").unwrap();
/// let shown = path.clone();
/// let app = App::new(move || spot(&shown));
/// let screen = app.render_with(&[], 60, 9).unwrap();
/// assert!(screen[0].starts_with("Name") && screen[0].contains("intuituive-spot-doc"));
/// assert!(screen.iter().any(|r| r.starts_with("Size") && r.contains("5B")), "{screen:?}");
/// # std::fs::remove_file(&path).ok();
/// ```
pub fn spot(path: impl AsRef<Path>) -> Node {
    facts(spot_facts(path.as_ref()))
}

/// What [`spot`] lists about `path`: each key and its value, in console
/// markup.
pub fn spot_facts(path: &Path) -> Vec<(String, String)> {
    let name = path.file_name().map_or_else(
        || path.display().to_string(),
        |n| n.to_string_lossy().into_owned(),
    );
    let entry = match path.parent() {
        Some(dir) => Entry::read(dir, name.clone()),
        None => Entry::read(path, ""),
    };
    let mime = if entry.dir {
        "inode/directory"
    } else {
        filetype::mime_for(path).unwrap_or_else(|_| entry.mime_by_name())
    };
    let size = if entry.dir {
        readable_size(entry.size)
    } else {
        format!("{} ({} bytes)", readable_size(entry.size), entry.size)
    };
    let mut rows = vec![
        ("Name".to_string(), escape(&name)),
        ("Path".to_string(), escape(&path.display().to_string())),
        ("MIME".to_string(), escape(mime)),
        ("Size".to_string(), escape(&size)),
        ("Mode".to_string(), coloured_permissions(entry.mode)),
        ("Owner".to_string(), escape(&owner(entry.uid, entry.gid))),
    ];
    let full = |time| escape(&rich_ext::fs::local_time(time, "%Y-%m-%d %H:%M:%S"));
    if let Some(time) = entry.mtime {
        rows.push(("Modified".into(), full(time)));
    }
    if let Some(time) = entry.btime {
        rows.push(("Created".into(), full(time)));
    }
    if let Some(to) = &entry.link {
        let to = escape(&to.display().to_string());
        let note = if entry.orphan {
            " [red](missing)[/]"
        } else {
            ""
        };
        rows.push(("Target".into(), format!("{to}{note}")));
    }
    rows
}
