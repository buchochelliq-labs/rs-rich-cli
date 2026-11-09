//! Lossless semantic updates and replaceable frames share one ordered
//! mailbox. Only one lightweight proxy marker is outstanding.
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
        if let Some(key) = key_of(&update) {
            let before = self.queue.len();
            self.queue
                .retain(|(_, old)| key_of(&old.update) != Some(key));
            self.replaced += before - self.queue.len();
        }
        self.queue
            .push_back((self.document, Envelope { update, source }));
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
        eprintln!(
            "RICH_MEDIA {{\"stage\":\"delivery_queue\",\"waiting\":{},\"waiting_peak\":{},\"rgb_bytes\":{},\"rgb_peak\":{},\"replaced\":{}}}",
            self.waiting, self.high_water, self.retained, self.retained_high_water, self.replaced
        );
    }
}
fn key_of(update: &Update) -> Option<Option<i64>> {
    match update {
        Update::Frame(f) => Some(Some(f.node)),
        Update::Screen(_) => Some(None),
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
        q.push(Update::Page(Arc::new(crate::model::Page {
            document: 50,
            ..Default::default()
        })));
        let (_, first) = q.pop_envelope().unwrap();
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
            [
                "page/11/1",
                "done/1",
                "page/11/2",
                "page/22/3",
                "frame/3",
                "done/2"
            ]
        );
    }
}
