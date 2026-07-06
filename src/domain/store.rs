// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Thomas Volk

use super::project::Project;
use super::validation::{ValidationError, validate};

pub struct ProjectStore {
    active: Option<Project>,
    pending: Option<Project>,
}

impl ProjectStore {
    pub fn new() -> Self {
        ProjectStore {
            active: None,
            pending: None,
        }
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
    use super::super::project::*;
    use super::*;

    fn make_valid_project() -> Project {
        Project {
            header: Header {
                bpm: 120,
                loop_duration: 1920,
            },
            tracks: vec![Track {
                name: "piano".to_string(),
                channel: 1,
                instrument: 0,
                notes: vec![Note {
                    start_tick: 0,
                    duration: 480,
                    pitch: 60,
                    velocity: 80,
                }],
                pitch_bends: vec![],
            }],
        }
    }

    #[test]
    fn test_new_store_has_no_active() {
        let store = ProjectStore::new();
        assert!(store.active().is_none());
    }

    #[test]
    fn test_set_pending_valid() {
        let mut store = ProjectStore::new();
        assert!(store.set_pending(make_valid_project()).is_ok());
        assert!(store.active().is_none());
    }

    #[test]
    fn test_set_pending_invalid() {
        let mut store = ProjectStore::new();
        let invalid = Project {
            header: Header {
                bpm: 0,
                loop_duration: 1920,
            },
            tracks: vec![],
        };
        assert!(store.set_pending(invalid).is_err());
        assert!(store.active().is_none());
    }

    #[test]
    fn test_commit_pending_swap() {
        let mut store = ProjectStore::new();
        store.set_pending(make_valid_project()).unwrap();
        assert!(store.commit_pending());
        assert!(store.active().is_some());
    }

    #[test]
    fn test_commit_pending_no_pending() {
        let mut store = ProjectStore::new();
        assert!(!store.commit_pending());
        assert!(store.active().is_none());
    }

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
