//! One bounded optional-resource worker; neither a slow browser cache
//! read nor decoding holds the engine's command/event loop.
use crate::engine::ResourceReader;
use rich_intuituive::Picture;
use std::collections::{HashSet, VecDeque};
use std::sync::{Arc, Condvar, Mutex, mpsc};

type Request = (u64, String);
#[derive(Default)]
struct Queue {
    waiting: VecDeque<Request>,
    stopped: bool,
}
pub(crate) struct InlineResources {
    queue: Arc<(Mutex<Queue>, Condvar)>,
    done: mpsc::Receiver<(Request, Picture)>,
}
impl InlineResources {
    pub fn spawn(mut resource: Option<ResourceReader>) -> Self {
        let queue = Arc::new((Mutex::new(Queue::default()), Condvar::new()));
        // One executing read/decode and one completion, independent of
        // document churn. Pending requests hold URLs, never image bytes.
        let (send, done) = mpsc::sync_channel(1);
        let shared = queue.clone();
        std::thread::spawn(move || {
            loop {
                let request = {
                    let (lock, wake) = &*shared;
                    let mut q = lock.lock().unwrap();
                    while !q.stopped && q.waiting.is_empty() {
                        q = wake.wait(q).unwrap();
                    }
                    if q.stopped {
                        return;
                    }
                    q.waiting.pop_front().unwrap()
                };
                let measured = crate::diagnostics::enabled().then(std::time::Instant::now);
                if measured.is_some() {
                    eprintln!(
                        "RICH_MEDIA {{\"stage\":\"inline_start\",\"document\":{}}}",
                        request.0
                    );
                }
                let bytes = if let Some(data) = request.1.strip_prefix("data:") {
                    crate::tab::data_bytes(data)
                } else {
                    resource.as_mut().and_then(|read| read(&request.1).ok())
                };
                let Some(bytes) = bytes.filter(|b| b.len() <= 16 << 20) else {
                    continue;
                };
                // Inline resources need bounded decoded storage as well as a
                // bounded queue. Reject enormous source dimensions before decode.
                let Some((w, h)) = image::ImageReader::new(std::io::Cursor::new(&bytes))
                    .with_guessed_format()
                    .ok()
                    .and_then(|r| r.into_dimensions().ok())
                else {
                    continue;
                };
                if u64::from(w) * u64::from(h) > 4 * 1024 * 1024 {
                    continue;
                }
                if let Some(start) = measured {
                    eprintln!(
                        "RICH_MEDIA {{\"stage\":\"inline_resource\",\"document\":{},\"ns\":{},\"source_bytes\":{}}}",
                        request.0,
                        start.elapsed().as_nanos(),
                        bytes.len()
                    );
                }
                if let Ok(picture) = Picture::decode_animated(&bytes) {
                    if retained_bytes(&picture) > 16 << 20 {
                        continue;
                    }
                    if crate::diagnostics::enabled() {
                        eprintln!(
                            "RICH_MEDIA {{\"stage\":\"inline_decoded\",\"document\":{},\"retained_decoded_bytes\":{}}}",
                            request.0,
                            retained_bytes(&picture)
                        );
                    }
                    if send.send((request, picture)).is_err() {
                        return;
                    }
                }
            }
        });
        Self { queue, done }
    }
    pub fn retain(&self, document: u64, sources: &HashSet<String>) {
        self.queue
            .0
            .lock()
            .unwrap()
            .waiting
            .retain(|(doc, src)| *doc == document && sources.contains(src));
    }
    pub fn submit(&self, document: u64, src: String) -> bool {
        let mut q = self.queue.0.lock().unwrap();
        if q.waiting.len() >= 128
            || src
                .len()
                .saturating_add(q.waiting.iter().map(|(_, s)| s.len()).sum::<usize>())
                > 4 << 20
        {
            return false;
        }
        q.waiting.push_back((document, src));
        if crate::diagnostics::enabled() {
            eprintln!(
                "RICH_MEDIA {{\"stage\":\"inline_queue\",\"waiting\":{},\"source_url_bytes\":{}}}",
                q.waiting.len(),
                q.waiting.iter().map(|(_, s)| s.len()).sum::<usize>()
            );
        }
        self.queue.1.notify_one();
        true
    }
    pub fn take(&self) -> Option<(Request, Picture)> {
        self.done.try_recv().ok()
    }
}
impl Drop for InlineResources {
    fn drop(&mut self) {
        self.queue.0.lock().unwrap().stopped = true;
        self.queue.1.notify_one();
    }
}
pub(crate) fn retained_bytes(p: &Picture) -> usize {
    p.image().as_bytes().len()
        + p.frames().map_or(0, |frames| {
            frames.iter().map(|f| f.image.as_raw().len()).sum::<usize>()
        })
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn optional_waiting_urls_have_a_byte_budget_and_can_be_pruned() {
        let (start, started) = mpsc::sync_channel(1);
        let (release, gate) = mpsc::sync_channel(1);
        let loader = InlineResources::spawn(Some(Box::new(move |_| {
            start.send(()).unwrap();
            gate.recv().unwrap();
            Err(std::io::Error::other("fixture"))
        })));
        assert!(loader.submit(1, "first".into()));
        started
            .recv_timeout(std::time::Duration::from_secs(2))
            .unwrap();
        assert!(loader.submit(1, "x".repeat(4 << 20)));
        assert!(!loader.submit(1, "one byte over".into()));
        loader.retain(2, &HashSet::new());
        assert_eq!(loader.queue.0.lock().unwrap().waiting.len(), 0);
        release.send(()).unwrap();
    }
}
