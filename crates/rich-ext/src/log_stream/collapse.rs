//! Folding runs of identical records into the first and a count.

use std::borrow::Cow;

use super::Record;

/// A run of identical records: how many, and when the last one was.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Repeat {
    /// The records in the run, the shown first one included: at least 2.
    pub count: usize,
    /// The last one's time, as written.
    pub last_time: Option<String>,
    /// The last one's time, parsed (see [`Record::instant`]).
    pub last_instant: Option<i64>,
}

/// What to do with a record given to [`Collapser::push`].
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Push {
    /// The run this record ended, to show before it.
    pub ended: Option<Repeat>,
    /// Whether to show the record: `false` when it repeats the one before.
    pub show: bool,
}

/// Folds consecutive records that are the same but for their time (see
/// [`Record::same_as`]) into the first one and a [`Repeat`].
///
/// ```
/// use rich_ext::event::Value;
/// use rich_ext::log_stream::{Collapser, Record};
///
/// let at = |second: u32| {
///     Record::new("json")
///         .field("time", Value::String(format!("2026-10-18T10:00:{second:02}Z")))
///         .field("msg", Value::String("connection refused".into()))
/// };
/// let mut collapser = Collapser::new();
/// assert!(collapser.push(&at(0)).show);
/// assert!(!collapser.push(&at(1)).show);
/// assert!(!collapser.push(&at(2)).show);
/// let repeat = collapser.finish().unwrap();
/// assert_eq!(repeat.count, 3);
/// assert_eq!(repeat.last_time.as_deref(), Some("2026-10-18T10:00:02Z"));
/// ```
#[derive(Clone, Debug, Default)]
pub struct Collapser {
    head: Option<Record>,
    run: Repeat,
    folded: usize,
}

impl Collapser {
    pub fn new() -> Collapser {
        Collapser::default()
    }

    /// Take the next record: shown when it starts a new run, folded when it
    /// repeats the run's first.
    pub fn push(&mut self, record: &Record) -> Push {
        if self.head.as_ref().is_some_and(|head| head.same_as(record)) {
            self.run.count += 1;
            self.run.last_time = record.time().map(Cow::into_owned);
            self.run.last_instant = record.instant();
            self.folded += 1;
            return Push {
                ended: None,
                show: false,
            };
        }
        let ended = self.end();
        self.head = Some(record.clone());
        self.run.count = 1;
        Push { ended, show: true }
    }

    /// The run so far, when it has repeats; it goes on.
    pub fn current(&self) -> Option<Repeat> {
        (self.run.count > 1).then(|| self.run.clone())
    }

    /// End the run (at the end of the input): its repeats, if it had any.
    /// The next record starts a new run whatever it is.
    pub fn finish(&mut self) -> Option<Repeat> {
        let ended = self.end();
        self.head = None;
        self.run.count = 0;
        ended
    }

    fn end(&mut self) -> Option<Repeat> {
        let run = std::mem::take(&mut self.run);
        (run.count > 1).then_some(run)
    }

    /// How many records were folded into the one before them.
    pub fn folded(&self) -> usize {
        self.folded
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::event::Value;

    fn record(message: &str, second: u32) -> Record {
        Record::new("json")
            .field(
                "time",
                Value::String(format!("2026-10-18T10:00:{second:02}Z")),
            )
            .field("level", Value::String("warn".into()))
            .field("msg", Value::String(message.into()))
    }

    #[test]
    fn runs_fold_and_end_on_the_next_record() {
        let mut collapser = Collapser::new();
        let mut shown = Vec::new();
        let mut repeats = Vec::new();
        for (message, second) in [("a", 0), ("a", 1), ("a", 2), ("b", 3), ("a", 4), ("a", 5)] {
            let push = collapser.push(&record(message, second));
            repeats.extend(push.ended);
            if push.show {
                shown.push((message, second));
            }
        }
        repeats.extend(collapser.finish());
        assert_eq!(shown, [("a", 0), ("b", 3), ("a", 4)]);
        assert_eq!(repeats.iter().map(|r| r.count).collect::<Vec<_>>(), [3, 2]);
        assert_eq!(
            repeats[0].last_time.as_deref(),
            Some("2026-10-18T10:00:02Z")
        );
        assert!(repeats[1].last_instant.is_some());
        assert_eq!(collapser.folded(), 3);
    }

    #[test]
    fn the_current_run_shows_without_ending_it() {
        let mut collapser = Collapser::new();
        collapser.push(&record("a", 0));
        assert_eq!(collapser.current(), None);
        collapser.push(&record("a", 1));
        assert_eq!(collapser.current().map(|r| r.count), Some(2));
        collapser.push(&record("a", 2));
        assert_eq!(collapser.current().map(|r| r.count), Some(3));
        assert_eq!(collapser.finish().map(|r| r.count), Some(3));
        // After the end, the same record starts over.
        assert!(collapser.push(&record("a", 3)).show);
        assert_eq!(collapser.finish(), None);
    }

    #[test]
    fn records_differing_in_more_than_time_do_not_fold() {
        let mut collapser = Collapser::new();
        assert!(collapser.push(&record("a", 0)).show);
        let mut other = record("a", 0);
        other.continuation.push("  at main".into());
        assert!(collapser.push(&other).show);
        assert!(
            collapser
                .push(&record("a", 0).field("level", Value::String("error".into())))
                .show
        );
    }
}
