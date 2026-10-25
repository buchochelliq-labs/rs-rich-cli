//! [`DraftField`]: a value edited here while the page holds the real one,
//! with a guarded commit and a conflict check. C5 of the design note: it
//! stays in Noctiluca until a second user needs it. A port of the proof
//! of concept's `primitives/draft-field.js`.
//!
//! The page's value is *confirmed*; what the user typed is the *draft*.
//! A commit sends the draft with the confirmed value it was based on, and
//! is refused if the page's value moved meanwhile. A change the page makes
//! under an edited draft is a *conflict*: the draft is kept, never
//! overwritten, and nothing is applied until the user reloads.

/// Where a draft is.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum DraftState<T> {
    /// The draft is the confirmed value.
    Clean,
    /// Edited, not sent.
    Dirty,
    /// Sent; waiting for the page.
    Pending,
    /// The page changed the value under the edit: this is the page's.
    Conflict { remote: T },
    /// The page refused the commit.
    Failed(String),
}

/// A value edited here, committed to the page with a check.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DraftField<T> {
    confirmed: T,
    draft: T,
    revision: u32,
    observed: T,
    observed_revision: u32,
    state: DraftState<T>,
}

/// A commit to send: the value, and the confirmed value it replaces.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Commit<T> {
    pub value: T,
    pub base: T,
    pub revision: u32,
}

impl<T: Clone + PartialEq> DraftField<T> {
    /// A clean field holding the page's `value`, at the page's `revision`.
    pub fn new(value: T, revision: u32) -> DraftField<T> {
        DraftField {
            confirmed: value.clone(),
            draft: value.clone(),
            revision,
            observed: value,
            observed_revision: revision,
            state: DraftState::Clean,
        }
    }

    pub fn state(&self) -> &DraftState<T> {
        &self.state
    }

    pub fn draft(&self) -> &T {
        &self.draft
    }

    pub fn confirmed(&self) -> &T {
        &self.confirmed
    }

    pub fn is_dirty(&self) -> bool {
        self.draft != self.confirmed
    }

    /// Edit the draft. Refused while a commit is pending.
    pub fn set(&mut self, value: T) -> Result<(), String> {
        if self.state == DraftState::Pending {
            return Err("A commit is already pending.".into());
        }
        self.draft = value;
        if !matches!(self.state, DraftState::Conflict { .. }) {
            self.state = if self.is_dirty() {
                DraftState::Dirty
            } else {
                DraftState::Clean
            };
        }
        Ok(())
    }

    /// The page's value now. A clean field follows it; an edited one keeps
    /// its draft and is in conflict if the page's value moved.
    pub fn observe(&mut self, value: T, revision: u32) {
        self.observed = value.clone();
        self.observed_revision = revision;
        if value == self.confirmed {
            if matches!(self.state, DraftState::Conflict { .. }) {
                // The page went back to what the edit was based on.
                self.state = if self.is_dirty() {
                    DraftState::Dirty
                } else {
                    DraftState::Clean
                };
            }
            self.revision = revision;
            return;
        }
        match self.state {
            // Only the commit's answer settles a pending commit.
            DraftState::Pending => {}
            DraftState::Clean | DraftState::Failed(_) if !self.is_dirty() => {
                self.accept(value, revision)
            }
            _ => self.state = DraftState::Conflict { remote: value },
        }
    }

    /// Start a commit: what to send. Refused in conflict or while pending.
    pub fn request(&mut self) -> Result<Commit<T>, String> {
        match &self.state {
            DraftState::Conflict { .. } => {
                Err("Edit conflict: the page changed this value. Reload it before applying.".into())
            }
            DraftState::Pending => Err("A commit is already pending.".into()),
            _ => {
                self.state = DraftState::Pending;
                Ok(Commit {
                    value: self.draft.clone(),
                    base: self.confirmed.clone(),
                    revision: self.revision,
                })
            }
        }
    }

    /// The page took `value`: it is confirmed.
    pub fn accept(&mut self, value: T, revision: u32) {
        self.confirmed = value.clone();
        self.draft = value.clone();
        self.observed = value;
        self.revision = revision;
        self.observed_revision = revision;
        self.state = DraftState::Clean;
    }

    /// The page refused the commit; with `remote`, because its value had
    /// moved to that.
    pub fn reject(&mut self, message: impl Into<String>, remote: Option<T>) {
        self.state = match remote {
            Some(remote) => DraftState::Conflict { remote },
            None => DraftState::Failed(message.into()),
        };
    }

    /// Drop the draft for the page's value now.
    pub fn reload(&mut self) {
        let (value, revision) = (self.observed.clone(), self.observed_revision);
        self.accept(value, revision);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_clean_field_follows_the_page() {
        let mut field = DraftField::new("a".to_string(), 1);
        field.observe("b".into(), 2);
        assert_eq!(
            (field.draft().as_str(), field.state()),
            ("b", &DraftState::Clean)
        );
    }

    #[test]
    fn an_edit_is_never_overwritten_by_the_page() {
        let mut field = DraftField::new("start".to_string(), 1);
        field.set("mine".into()).unwrap();
        assert_eq!(field.state(), &DraftState::Dirty);
        field.observe("remote".into(), 2);
        assert_eq!(
            field.state(),
            &DraftState::Conflict {
                remote: "remote".into()
            }
        );
        assert_eq!(field.draft(), "mine");
        assert!(field.request().unwrap_err().starts_with("Edit conflict"));
        field.reload();
        assert_eq!(
            (field.draft().as_str(), field.state()),
            ("remote", &DraftState::Clean)
        );
    }

    #[test]
    fn a_commit_carries_its_base_and_waits_for_the_page() {
        let mut field = DraftField::new(1, 1);
        field.set(5).unwrap();
        let commit = field.request().unwrap();
        assert_eq!((commit.value, commit.base), (5, 1));
        assert!(field.set(6).is_err());
        assert!(field.request().is_err());
        // The page's echo of the old value does not settle it.
        field.observe(3, 2);
        assert_eq!(field.state(), &DraftState::Pending);
        field.accept(5, 3);
        assert_eq!((field.draft(), field.state()), (&5, &DraftState::Clean));
    }

    #[test]
    fn a_refused_commit_says_why() {
        let mut field = DraftField::new(1, 1);
        field.set(2).unwrap();
        field.request().unwrap();
        field.reject("no", None);
        assert_eq!(field.state(), &DraftState::Failed("no".into()));
        field.request().unwrap();
        field.reject("moved", Some(9));
        assert_eq!(field.state(), &DraftState::Conflict { remote: 9 });
    }
}
