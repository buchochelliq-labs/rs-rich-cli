//! Lossless semantic updates and replaceable ones share one ordered
//! mailbox. Only one lightweight proxy marker is outstanding.
//!
//! Replaceable: a region's frame, the pixel view's frame, the page (each
//! reading is the whole page, with its source pair), the tab's state and
//! its scroll metrics; a newer one takes the older one's place at the back
//! of the queue, so a UI thread that falls behind (a slow terminal) finds
//! one of each, not a pile of whole pages. Command results, downloads,
//! stills and the about page's answers are never dropped.
use crate::model::SourcePage;
use crate::tab::Update;

pub(crate) struct Envelope {
    pub update: Update,
    pub source: Option<SourcePage>,
}

use std::collections::VecDeque;
#[derive(Default)]
pub(crate) struct Mailbox {
    pub queue: VecDeque<(u64, Envelope)>,
    document: u64,
    pub high_water: usize,
    pub retained_high_water: usize,
    pub replaced: usize,
}
impl Mailbox {
    #[cfg(test)]
    pub fn push(&mut self, update: Update) {
        self.push_source(update, None);
    }
    pub fn push_source(&mut self, update: Update, source: Option<SourcePage>) {
        assert!(source.as_ref().is_none_or(
            |s| matches!(&update, Update::Page(p) if std::sync::Arc::ptr_eq(p, s.page()))
        ));
        if let Update::Page(p) = &update {
            self.document = p.document;
        }
        // A page moving to the back takes its document's waiting frames
        // with it, so they still follow it: before it, the app would take
        // them for an earlier document's and drop them.
        let mut follow = Vec::new();
        if let Some(key) = key_of(&update) {
            let before = self.queue.len();
            if key == Key::Page
                && let Some(at) = self
                    .queue
                    .iter()
                    .position(|(_, old)| key_of(&old.update) == Some(Key::Page))
            {
                let document = self.document;
                let mut index = at + 1;
                while index < self.queue.len() {
                    let (tag, old) = &self.queue[index];
                    if *tag == document
                        && matches!(old.update, Update::Frame(_) | Update::Screen(_))
                    {
                        follow.extend(self.queue.remove(index));
                    } else {
                        index += 1;
                    }
                }
            }
            self.queue
                .retain(|(_, old)| key_of(&old.update) != Some(key));
            self.replaced += before - follow.len() - self.queue.len();
        }
        self.queue
            .push_back((self.document, Envelope { update, source }));
        self.queue.extend(follow);
        self.high_water = self.high_water.max(self.queue.len());
        let retained = self.queue.iter().map(|(_, u)| bytes(&u.update)).sum();
        self.retained_high_water = self.retained_high_water.max(retained);
    }
    pub fn trace(&self) -> Trace {
        Trace {
            waiting: self.queue.len(),
            high_water: self.high_water,
            retained: self.queue.iter().map(|(_, u)| bytes(&u.update)).sum(),
            retained_high_water: self.retained_high_water,
            replaced: self.replaced,
        }
    }

    pub fn pop_envelope(&mut self) -> Option<(u64, Envelope)> {
        self.queue.pop_front()
    }
    #[cfg(test)]
    pub fn pop(&mut self) -> Option<(u64, Update)> {
        self.pop_envelope()
            .map(|(document, event)| (document, event.update))
    }
}
pub(crate) struct Trace {
    waiting: usize,
    high_water: usize,
    retained: usize,
    retained_high_water: usize,
    replaced: usize,
}
impl Trace {
    pub fn emit(self) {
        crate::trace::media(&format!(
            "{{\"stage\":\"delivery_queue\",\"waiting\":{},\"waiting_peak\":{},\"rgb_bytes\":{},\"rgb_peak\":{},\"replaced\":{}}}",
            self.waiting, self.high_water, self.retained, self.retained_high_water, self.replaced
        ));
    }
}
/// What a replaceable update replaces: the waiting one of the same key.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Key {
    Frame(i64),
    Screen,
    Page,
    State,
    Metrics,
}

fn key_of(update: &Update) -> Option<Key> {
    match update {
        Update::Frame(f) => Some(Key::Frame(f.node)),
        Update::Screen(_) => Some(Key::Screen),
        Update::Page(_) => Some(Key::Page),
        Update::State(_) => Some(Key::State),
        Update::Metrics(_) => Some(Key::Metrics),
        _ => None,
    }
}
fn bytes(update: &Update) -> usize {
    match update {
        Update::Frame(f) => f.pixels.rgb.len(),
        Update::Screen(f) => f.pixels.rgb.len(),
        _ => 0,
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;
    fn frame(n: u64) -> Update {
        Update::Frame(Arc::new(crate::video::VideoFrame {
            node: 7,
            seq: n,
            pixels: rich_embed::Pixels::new(2, 2, vec![n as u8; 12]).unwrap(),
        }))
    }
    #[test]
    fn source_envelope_keeps_the_exact_page_across_navigation_and_legacy_updates() {
        let snapshot =
            crate::testing::PageBuilder::new("https://example.test/", "pair", 400.0, 400.0)
                .snapshot();
        let source = crate::model::build_source(&snapshot, &mut crate::model::Revisions::new());
        let mut q = Mailbox::default();
        q.push_source(Update::Page(source.page().clone()), Some(source.clone()));
        let (_, first) = q.pop_envelope().unwrap();
        q.push(Update::Page(Arc::new(crate::model::Page {
            document: 50,
            ..Default::default()
        })));
        let Update::Page(page) = first.update else {
            panic!("page expected")
        };
        assert!(Arc::ptr_eq(&page, first.source.unwrap().page()));
        assert!(Arc::ptr_eq(&page, source.page()));
        let (document, legacy) = q.pop_envelope().unwrap();
        assert_eq!(document, 50);
        assert!(legacy.source.is_none());
    }

    #[test]
    fn waiting_pages_coalesce_to_the_newest_with_its_source_and_results_stay() {
        let snapshot =
            crate::testing::PageBuilder::new("https://example.test/", "pair", 400.0, 400.0)
                .snapshot();
        let mut q = Mailbox::default();
        let mut newest = None;
        for n in 0..50u64 {
            let source = crate::model::build_source(&snapshot, &mut crate::model::Revisions::new());
            q.push_source(Update::Page(source.page().clone()), Some(source.clone()));
            q.push(Update::Done(n, Ok(())));
            q.push(Update::Metrics(crate::engine::Metrics::default()));
            newest = Some(source);
        }
        // Fifty results, one page (the last, with its own source) and one
        // metrics: the queue stays bounded by what must not be lost.
        assert_eq!(q.queue.len(), 52);
        let mut done = Vec::new();
        let mut pages = 0;
        while let Some((_, envelope)) = q.pop_envelope() {
            match envelope.update {
                Update::Done(n, _) => done.push(n),
                Update::Page(page) => {
                    pages += 1;
                    let source = envelope.source.unwrap();
                    assert!(Arc::ptr_eq(&page, source.page()));
                    assert!(Arc::ptr_eq(&page, newest.as_ref().unwrap().page()));
                    // After every result queued before it.
                    assert_eq!(done.len(), 49);
                }
                Update::Metrics(_) => {}
                _ => panic!("unexpected update"),
            }
        }
        assert_eq!(pages, 1);
        assert_eq!(done, (0..50).collect::<Vec<_>>());
    }

    #[test]
    fn a_coalesced_page_keeps_its_documents_frames_after_it() {
        // A frame queued after its document's page stays after it when a
        // newer reading of that page takes the waiting one's place: before
        // it, the app would drop the frame as an earlier document's.
        let page = |document| {
            Update::Page(Arc::new(crate::model::Page {
                document,
                ..Default::default()
            }))
        };
        let screen = || {
            Update::Screen(Arc::new(rich_embed::ScreencastFrame {
                pixels: rich_embed::Pixels::new(2, 2, vec![0; 12]).unwrap(),
                meta: Default::default(),
            }))
        };
        let mut q = Mailbox::default();
        q.push(page(5));
        q.push(screen());
        q.push(frame(1));
        q.push(Update::Done(1, Ok(())));
        q.push(page(5));
        let seen: Vec<&str> = std::iter::from_fn(|| q.pop())
            .map(|(document, update)| {
                assert_eq!(document, 5);
                match update {
                    Update::Page(_) => "page",
                    Update::Screen(_) => "screen",
                    Update::Frame(_) => "frame",
                    Update::Done(..) => "done",
                    _ => "other",
                }
            })
            .collect();
        assert_eq!(seen, ["done", "page", "screen", "frame"]);
        // A newer document's page leaves the earlier one's frames before it.
        let mut q = Mailbox::default();
        q.push(page(5));
        q.push(screen());
        q.push(page(6));
        let order: Vec<u64> = std::iter::from_fn(|| q.pop()).map(|(d, _)| d).collect();
        assert_eq!(order, [5, 6]);
    }
    #[test]
    fn frames_are_bounded_and_semantic_updates_are_lossless() {
        let mut q = Mailbox::default();
        for n in 0..100 {
            q.push(frame(n));
        }
        assert_eq!(q.queue.len(), 1);
        assert_eq!(q.high_water, 1);
        assert_eq!(q.retained_high_water, 12);
        q.push(Update::Done(1, Ok(())));
        q.push(frame(100));
        q.push(Update::Done(2, Ok(())));
        assert!(matches!(q.pop(), Some((_, Update::Done(1, _)))));
        assert!(matches!(q.pop(),Some((_,Update::Frame(f))) if f.seq==100));
        assert!(matches!(q.pop(), Some((_, Update::Done(2, _)))));
        assert_eq!(q.replaced, 100);
    }
    #[test]
    fn mixed_pages_logs_frames_and_results_keep_document_barriers() {
        let mut q = Mailbox::default();
        let page = |document, key| {
            let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("tests/snapshots/stream-with-log.json");
            let mut p = crate::model::build(
                &crate::testing::snapshot_file(&root).unwrap(),
                &mut crate::model::Revisions::new(),
            );
            p.document = document;
            p.logs[0].entries[0].key = key;
            Update::Page(Arc::new(p))
        };
        q.push(page(11, 1));
        q.push(frame(1));
        q.push(Update::Done(1, Ok(())));
        q.push(page(11, 2));
        q.push(frame(2));
        q.push(page(22, 3));
        q.push(frame(3));
        q.push(Update::Done(2, Ok(())));
        let mut seen = Vec::new();
        let mut current = 0;
        while let Some((document, update)) = q.pop() {
            match update {
                Update::Page(p) => {
                    current = p.document;
                    seen.push(format!("page/{}/{}", p.document, p.logs[0].entries[0].key));
                }
                Update::Done(id, _) => seen.push(format!("done/{id}")),
                Update::Frame(f) => {
                    assert_eq!(document, current);
                    assert_eq!(document, 22);
                    assert_eq!(f.seq, 3);
                    seen.push("frame/3".into());
                }
                _ => panic!("unexpected update"),
            }
        }
        assert_eq!(
            seen,
            // The waiting pages coalesce to the newest, after the result
            // queued before it; its frame follows it.
            ["done/1", "page/22/3", "frame/3", "done/2"]
        );
    }
}
