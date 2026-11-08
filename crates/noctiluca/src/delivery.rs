//! Lossless semantic updates and replaceable frames share one ordered
//! mailbox. Only one lightweight proxy marker is outstanding.
use crate::tab::Update;
use std::collections::VecDeque;
#[derive(Default)]
pub(crate) struct Mailbox {
    pub queue: VecDeque<(u64, Update)>,
    document: u64,
    pub high_water: usize,
    pub retained_high_water: usize,
    pub replaced: usize,
}
impl Mailbox {
    pub fn push(&mut self, update: Update) {
        if let Update::Page(p) = &update {
            self.document = p.document;
        }
        if let Some(key) = key_of(&update) {
            let before = self.queue.len();
            self.queue.retain(|(_, old)| key_of(old) != Some(key));
            self.replaced += before - self.queue.len();
        }
        self.queue.push_back((self.document, update));
        self.high_water = self.high_water.max(self.queue.len());
        let retained = self.queue.iter().map(|(_, u)| bytes(u)).sum();
        self.retained_high_water = self.retained_high_water.max(retained);
        if crate::diagnostics::enabled() {
            eprintln!(
                "RICH_MEDIA {{\"stage\":\"delivery_queue\",\"waiting\":{},\"waiting_peak\":{},\"rgb_bytes\":{},\"rgb_peak\":{},\"replaced\":{}}}",
                self.queue.len(),
                self.high_water,
                retained,
                self.retained_high_water,
                self.replaced
            );
        }
    }
    pub fn pop(&mut self) -> Option<(u64, Update)> {
        self.queue.pop_front()
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
