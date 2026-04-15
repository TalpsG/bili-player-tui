pub mod track;

use track::Track;

/// Playback queue. P1: sequential only. P2 adds shuffle/repeat/persistence.
#[derive(Debug, Default)]
pub struct Queue {
    tracks: Vec<Track>,
    current_index: Option<usize>,
}

impl Queue {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn is_empty(&self) -> bool {
        self.tracks.is_empty()
    }

    pub fn len(&self) -> usize {
        self.tracks.len()
    }

    pub fn tracks(&self) -> &[Track] {
        &self.tracks
    }

    pub fn current_index(&self) -> Option<usize> {
        self.current_index
    }

    pub fn current_track(&self) -> Option<&Track> {
        self.current_index.and_then(|i| self.tracks.get(i))
    }

    pub fn current_track_mut(&mut self) -> Option<&mut Track> {
        self.current_index.and_then(move |i| self.tracks.get_mut(i))
    }

    /// Add track to end of queue.
    pub fn push(&mut self, track: Track) {
        self.tracks.push(track);
        if self.current_index.is_none() {
            self.current_index = Some(0);
        }
    }

    /// Insert track after current, or at end if nothing playing.
    pub fn insert_next(&mut self, track: Track) {
        let insert_at = self
            .current_index
            .map(|i| i + 1)
            .unwrap_or(self.tracks.len());
        self.tracks.insert(insert_at, track);
        if self.current_index.is_none() {
            self.current_index = Some(0);
        }
    }

    /// Remove track at index. Adjusts current_index.
    pub fn remove(&mut self, index: usize) -> Option<Track> {
        if index >= self.tracks.len() {
            return None;
        }
        let track = self.tracks.remove(index);
        self.current_index = match self.current_index {
            Some(ci) if ci == index => {
                // Removed current track
                if self.tracks.is_empty() {
                    None
                } else if ci >= self.tracks.len() {
                    Some(self.tracks.len() - 1)
                } else {
                    Some(ci)
                }
            }
            Some(ci) if ci > index => Some(ci - 1),
            Some(ci) => Some(ci),
            None => None,
        };
        Some(track)
    }

    /// Clear all tracks, reset current_index.
    pub fn clear(&mut self) {
        self.tracks.clear();
        self.current_index = None;
    }

    /// Move to next track. Returns the new current track, or None if queue ended.
    pub fn next(&mut self) -> Option<&Track> {
        let next = match self.current_index {
            Some(ci) if ci + 1 < self.tracks.len() => Some(ci + 1),
            _ => None,
        };
        self.current_index = next;
        next.and_then(|i| self.tracks.get(i))
    }

    /// Move to previous track. Returns the new current track, or None if at start.
    /// If already at start, stays at current track.
    pub fn prev(&mut self) -> Option<&Track> {
        match self.current_index {
            Some(ci) if ci > 0 => {
                self.current_index = Some(ci - 1);
            }
            Some(_) => {
                // At start, don't move
            }
            None => {}
        }
        self.current_track()
    }

    /// Jump to a specific track by index.
    pub fn jump_to(&mut self, index: usize) -> Option<&Track> {
        if index < self.tracks.len() {
            self.current_index = Some(index);
            self.tracks.get(index)
        } else {
            None
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    fn test_track(name: &str) -> Track {
        Track {
            bvid: format!("BV{name}"),
            cid: 1,
            title: name.to_string(),
            author: "test".to_string(),
            duration: Duration::from_secs(180),
            cover_url: None,
            source: None,
        }
    }

    #[test]
    fn test_push_and_current() {
        let mut q = Queue::new();
        assert!(q.is_empty());
        assert_eq!(q.current_track(), None);

        q.push(test_track("A"));
        assert_eq!(q.len(), 1);
        assert_eq!(q.current_track().unwrap().title, "A");

        q.push(test_track("B"));
        assert_eq!(q.len(), 2);
        assert_eq!(q.current_track().unwrap().title, "A"); // still first
    }

    #[test]
    fn test_next_prev() {
        let mut q = Queue::new();
        q.push(test_track("A"));
        q.push(test_track("B"));
        q.push(test_track("C"));

        assert_eq!(q.current_track().unwrap().title, "A");

        let next = q.next();
        assert_eq!(next.unwrap().title, "B");

        let prev = q.prev();
        assert_eq!(prev.unwrap().title, "A");

        // prev at start stays at current
        assert_eq!(q.prev().unwrap().title, "A");
        assert_eq!(q.current_track().unwrap().title, "A");
    }

    #[test]
    fn test_next_at_end() {
        let mut q = Queue::new();
        q.push(test_track("A"));
        q.next();
        // At end of queue
        assert_eq!(q.next(), None);
        assert_eq!(q.current_track(), None);
    }

    #[test]
    fn test_jump_to() {
        let mut q = Queue::new();
        q.push(test_track("A"));
        q.push(test_track("B"));
        q.push(test_track("C"));

        let t = q.jump_to(2);
        assert_eq!(t.unwrap().title, "C");

        assert_eq!(q.jump_to(99), None);
    }

    #[test]
    fn test_remove_current() {
        let mut q = Queue::new();
        q.push(test_track("A"));
        q.push(test_track("B"));
        q.push(test_track("C"));
        q.jump_to(1); // current = B

        let removed = q.remove(1);
        assert_eq!(removed.unwrap().title, "B");
        assert_eq!(q.len(), 2);
        // current_index stays at 1, which is now C
        assert_eq!(q.current_track().unwrap().title, "C");
    }

    #[test]
    fn test_remove_before_current() {
        let mut q = Queue::new();
        q.push(test_track("A"));
        q.push(test_track("B"));
        q.push(test_track("C"));
        q.jump_to(2); // current = C

        q.remove(0); // remove A
        assert_eq!(q.current_index(), Some(1));
        assert_eq!(q.current_track().unwrap().title, "C");
    }

    #[test]
    fn test_remove_last_when_current() {
        let mut q = Queue::new();
        q.push(test_track("A"));
        q.push(test_track("B"));
        q.jump_to(1); // current = B (last)

        q.remove(1);
        assert_eq!(q.current_index(), Some(0));
        assert_eq!(q.current_track().unwrap().title, "A");
    }

    #[test]
    fn test_remove_only_track() {
        let mut q = Queue::new();
        q.push(test_track("A"));
        q.remove(0);
        assert!(q.is_empty());
        assert_eq!(q.current_index(), None);
    }

    #[test]
    fn test_clear() {
        let mut q = Queue::new();
        q.push(test_track("A"));
        q.push(test_track("B"));
        q.clear();
        assert!(q.is_empty());
        assert_eq!(q.current_index(), None);
    }

    #[test]
    fn test_insert_next() {
        let mut q = Queue::new();
        q.push(test_track("A"));
        q.push(test_track("C"));
        q.jump_to(0); // current = A

        q.insert_next(test_track("B")); // insert after A
        assert_eq!(q.tracks().iter().map(|t| t.title.clone()).collect::<Vec<_>>(), vec!["A", "B", "C"]);
        assert_eq!(q.current_index(), Some(0));
    }
}
