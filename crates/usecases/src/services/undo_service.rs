//! Undo/Redo service using command pattern

use snapshort_domain::prelude::*;
use std::collections::VecDeque;

/// Maximum undo history size
const MAX_UNDO_HISTORY: usize = 100;

/// A snapshot of timeline state for undo
#[derive(Debug, Clone)]
pub struct UndoEntry {
    pub description: String,
    pub timeline_snapshot: Timeline,
}

/// Manages undo/redo history for a timeline
pub struct UndoService {
    history: VecDeque<UndoEntry>,
    redo_stack: Vec<UndoEntry>,
    current: Option<Timeline>,
}

impl UndoService {
    pub fn new() -> Self {
        Self {
            history: VecDeque::new(),
            redo_stack: Vec::new(),
            current: None,
        }
    }

    /// Initialize with a timeline
    pub fn init(&mut self, timeline: Timeline) {
        self.current = Some(timeline);
        self.history.clear();
        self.redo_stack.clear();
    }

    /// Push a new state (after an operation)
    pub fn push(&mut self, description: impl Into<String>, new_state: Timeline) {
        if let Some(current) = self.current.take() {
            // Save current as history
            self.history.push_back(UndoEntry {
                description: description.into(),
                timeline_snapshot: current,
            });

            // Limit history size
            while self.history.len() > MAX_UNDO_HISTORY {
                self.history.pop_front();
            }

            // Clear redo stack on new action
            self.redo_stack.clear();
        }

        self.current = Some(new_state);
    }

    /// Undo last operation
    pub fn undo(&mut self) -> Option<Timeline> {
        let previous = self.history.pop_back()?;

        if let Some(current) = self.current.take() {
            self.redo_stack.push(UndoEntry {
                description: previous.description.clone(),
                timeline_snapshot: current,
            });
        }

        self.current = Some(previous.timeline_snapshot.clone());
        self.current.clone()
    }

    /// Redo last undone operation
    pub fn redo(&mut self) -> Option<Timeline> {
        let next = self.redo_stack.pop()?;

        if let Some(current) = self.current.take() {
            self.history.push_back(UndoEntry {
                description: next.description.clone(),
                timeline_snapshot: current,
            });
        }

        self.current = Some(next.timeline_snapshot.clone());
        self.current.clone()
    }

    /// Check if undo is available
    pub fn can_undo(&self) -> bool {
        !self.history.is_empty()
    }

    /// Check if redo is available
    pub fn can_redo(&self) -> bool {
        !self.redo_stack.is_empty()
    }

    /// Get current state
    pub fn current(&self) -> Option<&Timeline> {
        self.current.as_ref()
    }

    /// Get undo history descriptions
    pub fn undo_descriptions(&self) -> Vec<&str> {
        self.history.iter().rev().map(|e| e.description.as_str()).collect()
    }

    /// Get redo history descriptions
    pub fn redo_descriptions(&self) -> Vec<&str> {
        self.redo_stack.iter().rev().map(|e| e.description.as_str()).collect()
    }
}

impl Default for UndoService {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_undo_redo() {
        let mut undo = UndoService::new();

        let t1 = Timeline::new("V1");
        undo.init(t1.clone());

        let t2 = t1.clone().seek(Frame(100));
        undo.push("Seek to 100", t2.clone());

        let t3 = t2.clone().seek(Frame(200));
        undo.push("Seek to 200", t3.clone());

        // Current should be t3
        assert_eq!(undo.current().unwrap().playhead.0, 200);

        // Undo to t2
        let undone = undo.undo().unwrap();
        assert_eq!(undone.playhead.0, 100);

        // Redo back to t3
        let redone = undo.redo().unwrap();
        assert_eq!(redone.playhead.0, 200);
    }

    #[test]
    fn test_redo_cleared_on_new_action() {
        let mut undo = UndoService::new();

        let t1 = Timeline::new("V1");
        undo.init(t1.clone());
        undo.push("First", t1.clone().seek(Frame(100)));
        undo.push("Second", t1.clone().seek(Frame(200)));

        undo.undo(); // Go back to 100
        assert!(undo.can_redo());

        // New action should clear redo
        undo.push("New action", t1.clone().seek(Frame(50)));
        assert!(!undo.can_redo());
    }
}
