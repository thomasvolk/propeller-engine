use super::project::Project;
use super::validation::{validate, ValidationError};

pub struct ProjectStore {
    active: Option<Project>,
    pending: Option<Project>,
}

impl ProjectStore {
    pub fn new() -> Self {
        ProjectStore { active: None, pending: None }
    }

    pub fn active(&self) -> Option<&Project> {
        self.active.as_ref()
    }

    pub fn set_pending(&mut self, project: Project) -> Result<(), ValidationError> {
        validate(&project)?;
        self.pending = Some(project);
        Ok(())
    }

    pub fn commit_pending(&mut self) -> bool {
        if self.pending.is_some() {
            self.active = self.pending.take();
            true
        } else {
            false
        }
    }

    #[cfg(test)]
    pub fn clear(&mut self) {
        self.active = None;
        self.pending = None;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use super::super::project::*;

    fn make_valid_project() -> Project {
        Project {
            header: Header {
                bpm: 120,
                time_signature: TimeSignature { numerator: 4, denominator: 4 },
            },
            tracks: vec![Track {
                name: "piano".to_string(),
                channel: 1,
                instrument: 0,
                bars: vec![Bar {
                    notes: vec![Note {
                        event: NoteEvent::Note { pitch: 60, velocity: 80 },
                        duration_ticks: 480,
                    }],
                }],
            }],
        }
    }

    // T-17: new ProjectStore has active = None
    #[test]
    fn test_new_store_has_no_active() {
        let store = ProjectStore::new();
        assert!(store.active().is_none());
    }

    // T-19: set_pending with valid project returns Ok; active still None
    #[test]
    fn test_set_pending_valid() {
        let mut store = ProjectStore::new();
        assert!(store.set_pending(make_valid_project()).is_ok());
        assert!(store.active().is_none());
    }

    // T-20: set_pending with invalid project returns Err; active unchanged
    #[test]
    fn test_set_pending_invalid() {
        let mut store = ProjectStore::new();
        let invalid = Project {
            header: Header {
                bpm: 0,
                time_signature: TimeSignature { numerator: 4, denominator: 4 },
            },
            tracks: vec![],
        };
        assert!(store.set_pending(invalid).is_err());
        assert!(store.active().is_none());
    }

    // T-22: commit_pending moves pending to active, clears pending, returns true
    #[test]
    fn test_commit_pending_swap() {
        let mut store = ProjectStore::new();
        store.set_pending(make_valid_project()).unwrap();
        assert!(store.commit_pending());
        assert!(store.active().is_some());
    }

    // T-23: commit_pending with no pending is no-op, returns false
    #[test]
    fn test_commit_pending_no_pending() {
        let mut store = ProjectStore::new();
        assert!(!store.commit_pending());
        assert!(store.active().is_none());
    }

    // T-25: set_pending twice retains only the second (most recent) project
    #[test]
    fn test_set_pending_twice_retains_last() {
        let mut store = ProjectStore::new();
        let first = make_valid_project();
        let mut second = make_valid_project();
        second.header.bpm = 140;

        store.set_pending(first).unwrap();
        store.set_pending(second).unwrap();
        store.commit_pending();

        assert_eq!(store.active().unwrap().header.bpm, 140);
    }
}
