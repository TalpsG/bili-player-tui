pub mod track;

use rand::seq::SliceRandom;

use track::Track;

/// Playback mode. Cycled by the `r` key.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PlayMode {
    #[default]
    Sequential,  // play in order, stop at end
    RepeatList,  // loop back to first track at end
    RepeatOne,   // replay current track on end
    Shuffle,     // play shuffle_order, re-shuffle when exhausted
}

impl PlayMode {
    /// Cycle to the next mode.
    pub fn next(self) -> Self {
        match self {
            Self::Sequential => Self::RepeatList,
            Self::RepeatList => Self::RepeatOne,
            Self::RepeatOne  => Self::Shuffle,
            Self::Shuffle    => Self::Sequential,
        }
    }

    /// Status bar icon. Empty string for Sequential.
    pub fn icon(self) -> &'static str {
        match self {
            Self::Sequential => "",
            Self::RepeatList => "🔁",
            Self::RepeatOne  => "🔂",
            Self::Shuffle    => "🔀",
        }
    }
}

/// Playback queue. P1: sequential only. P2 adds shuffle/repeat/persistence.
#[derive(Debug, Default)]
pub struct Queue {
    tracks: Vec<Track>,
    current_index: Option<usize>,
    pub play_mode: PlayMode,
    shuffle_order: Vec<usize>, // indices into tracks[], pre-generated permutation
    shuffle_pos: usize,        // current position within shuffle_order
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
        let new_idx = self.tracks.len();
        self.tracks.push(track);
        if self.current_index.is_none() {
            self.current_index = Some(0);
        }
        // In shuffle mode, append new index after current shuffle position
        if self.play_mode == PlayMode::Shuffle
            && self.shuffle_order.len() < self.tracks.len()
        {
            // Insert new index right after current shuffle_pos
            let insert_at = (self.shuffle_pos + 1).min(self.shuffle_order.len());
            self.shuffle_order.insert(insert_at, new_idx);
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
        self.shuffle_order.clear();
        self.shuffle_pos = 0;
    }

    /// Advance to the next track according to play_mode.
    /// Returns the new current track, or None if playback should stop.
    pub fn advance(&mut self) -> Option<&Track> {
        match self.play_mode {
            PlayMode::Sequential => {
                let next_idx = self.current_index.map(|i| i + 1)?;
                if next_idx < self.tracks.len() {
                    self.current_index = Some(next_idx);
                    self.tracks.get(next_idx)
                } else {
                    self.current_index = None;
                    None
                }
            }
            PlayMode::RepeatList => {
                if self.tracks.is_empty() {
                    return None;
                }
                let next_idx = match self.current_index {
                    Some(i) if i + 1 < self.tracks.len() => i + 1,
                    _ => 0,
                };
                self.current_index = Some(next_idx);
                self.tracks.get(next_idx)
            }
            PlayMode::RepeatOne => {
                // Stay on same track
                self.current_index.and_then(|i| self.tracks.get(i))
            }
            PlayMode::Shuffle => {
                if self.tracks.is_empty() {
                    return None;
                }
                // Ensure shuffle_order is populated
                if self.shuffle_order.len() != self.tracks.len() {
                    self.regenerate_shuffle_order();
                }
                self.shuffle_pos += 1;
                if self.shuffle_pos >= self.shuffle_order.len() {
                    // Exhausted — re-shuffle and start over
                    self.regenerate_shuffle_order();
                    self.shuffle_pos = 0;
                }
                let idx = self.shuffle_order[self.shuffle_pos];
                self.current_index = Some(idx);
                self.tracks.get(idx)
            }
        }
    }

    /// Go to previous track according to play_mode.
    pub fn prev(&mut self) -> Option<&Track> {
        match self.play_mode {
            PlayMode::RepeatOne => {
                // Stay on same track
                self.current_index.and_then(|i| self.tracks.get(i))
            }
            PlayMode::Shuffle => {
                if self.tracks.is_empty() {
                    return None;
                }
                if self.shuffle_order.len() != self.tracks.len() {
                    self.regenerate_shuffle_order();
                }
                if self.shuffle_pos > 0 {
                    self.shuffle_pos -= 1;
                }
                let idx = self.shuffle_order[self.shuffle_pos];
                self.current_index = Some(idx);
                self.tracks.get(idx)
            }
            _ => {
                // Sequential and RepeatList: go back one, stop at 0
                match self.current_index {
                    Some(ci) if ci > 0 => {
                        self.current_index = Some(ci - 1);
                    }
                    _ => {}
                }
                self.current_track()
            }
        }
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

    /// Cycle to the next PlayMode. Initialises shuffle_order when entering Shuffle mode.
    pub fn cycle_play_mode(&mut self) {
        self.play_mode = self.play_mode.next();
        if self.play_mode == PlayMode::Shuffle && !self.tracks.is_empty() {
            self.regenerate_shuffle_order();
            // Set shuffle_pos to the position of current_index in the new order (if present)
            if let Some(current) = self.current_index
                && let Some(pos) = self.shuffle_order.iter().position(|&i| i == current)
            {
                self.shuffle_pos = pos;
            }
        }
    }

    /// Return a snapshot suitable for persistence.
    /// `source` fields are skipped (they are `#[serde(skip)]` on `Track` already).
    pub fn snapshot(&self) -> (Vec<Track>, Option<usize>, PlayMode) {
        (self.tracks.clone(), self.current_index, self.play_mode)
    }

    /// Restore queue from a persisted snapshot.
    /// shuffle_order/shuffle_pos are regenerated lazily on first `next()` call.
    pub fn restore(tracks: Vec<Track>, current_index: Option<usize>, play_mode: PlayMode) -> Self {
        let current_index = current_index.filter(|&i| i < tracks.len());
        let mut q = Self {
            tracks,
            current_index,
            play_mode,
            shuffle_order: Vec::new(),
            shuffle_pos: 0,
        };
        // Pre-generate shuffle order if we're restoring into Shuffle mode
        if play_mode == PlayMode::Shuffle && !q.tracks.is_empty() {
            q.regenerate_shuffle_order();
            if let Some(current) = current_index
                && let Some(pos) = q.shuffle_order.iter().position(|&i| i == current)
            {
                q.shuffle_pos = pos;
            }
        }
        q
    }

    fn regenerate_shuffle_order(&mut self) {
        let mut order: Vec<usize> = (0..self.tracks.len()).collect();
        order.shuffle(&mut rand::rng());
        // Avoid starting with the currently playing track
        if let Some(current) = self.current_index
            && order.first() == Some(&current)
            && order.len() > 1
        {
            order.swap(0, 1);
        }
        self.shuffle_order = order;
        self.shuffle_pos = 0;
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

        let next = q.advance();
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
        q.advance();
        // At end of queue
        assert_eq!(q.advance(), None);
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

    #[test]
    fn test_play_mode_default() {
        let q = Queue::new();
        assert_eq!(q.play_mode, PlayMode::Sequential);
    }

    #[test]
    fn test_play_mode_cycle() {
        assert_eq!(PlayMode::Sequential.next(), PlayMode::RepeatList);
        assert_eq!(PlayMode::RepeatList.next(), PlayMode::RepeatOne);
        assert_eq!(PlayMode::RepeatOne.next(), PlayMode::Shuffle);
        assert_eq!(PlayMode::Shuffle.next(), PlayMode::Sequential);
    }

    #[test]
    fn test_play_mode_icons() {
        assert_eq!(PlayMode::Sequential.icon(), "");
        assert_eq!(PlayMode::RepeatList.icon(), "🔁");
        assert_eq!(PlayMode::RepeatOne.icon(), "🔂");
        assert_eq!(PlayMode::Shuffle.icon(), "🔀");
    }

    #[test]
    fn test_repeat_list_wraps() {
        let mut q = Queue::new();
        q.push(test_track("A"));
        q.push(test_track("B"));
        q.push(test_track("C"));
        q.cycle_play_mode(); // Sequential -> RepeatList

        // Advance to last track
        q.jump_to(2);
        // next should wrap back to 0
        let next = q.advance();
        assert_eq!(next.unwrap().title, "A");
        assert_eq!(q.current_index(), Some(0));
    }

    #[test]
    fn test_repeat_one_stays() {
        let mut q = Queue::new();
        q.push(test_track("A"));
        q.push(test_track("B"));
        q.jump_to(0);
        q.cycle_play_mode(); // Sequential -> RepeatList
        q.cycle_play_mode(); // RepeatList -> RepeatOne

        let next = q.advance();
        assert_eq!(next.unwrap().title, "A");
        let next2 = q.advance();
        assert_eq!(next2.unwrap().title, "A");
    }

    #[test]
    fn test_cycle_play_mode_enters_shuffle() {
        let mut q = Queue::new();
        q.push(test_track("A"));
        q.push(test_track("B"));
        q.push(test_track("C"));

        // Cycle to Shuffle
        q.cycle_play_mode(); // -> RepeatList
        q.cycle_play_mode(); // -> RepeatOne
        q.cycle_play_mode(); // -> Shuffle

        assert_eq!(q.play_mode, PlayMode::Shuffle);
        // shuffle_order should be populated
        assert_eq!(q.shuffle_order.len(), 3);
    }

    #[test]
    fn test_shuffle_covers_all_tracks() {
        let mut q = Queue::new();
        for name in ["A", "B", "C", "D", "E"] {
            q.push(test_track(name));
        }
        q.cycle_play_mode(); // -> RepeatList
        q.cycle_play_mode(); // -> RepeatOne
        q.cycle_play_mode(); // -> Shuffle

        // The correct invariant: shuffle_order must be a permutation of 0..n.
        // The previous approach of "advance 4 times and see all 5" was wrong:
        // if shuffle_pos starts at e.g. 3, advancing 2 exhausts the cycle and
        // reshuffles, causing already-seen indices to appear again within those
        // 4 steps — so seen.len() can be < 5 without any bug.
        let mut order = q.shuffle_order.clone();
        order.sort_unstable();
        assert_eq!(order, vec![0, 1, 2, 3, 4],
            "shuffle_order must contain each track index exactly once");
    }

    // ─── RepeatList ───────────────────────────────────────────────────────────

    #[test]
    fn test_repeat_list_single_track() {
        let mut q = Queue::new();
        q.push(test_track("A"));
        q.cycle_play_mode(); // Sequential -> RepeatList

        // advance() on the single track wraps back to itself
        let t = q.advance().unwrap();
        assert_eq!(t.title, "A");
        assert_eq!(q.current_index(), Some(0));
    }

    #[test]
    fn test_repeat_list_prev_at_start() {
        let mut q = Queue::new();
        q.push(test_track("A"));
        q.push(test_track("B"));
        q.push(test_track("C"));
        q.cycle_play_mode(); // Sequential -> RepeatList
        q.jump_to(0);

        // RepeatList does NOT wrap backward — stays at 0
        let t = q.prev().unwrap();
        assert_eq!(t.title, "A");
        assert_eq!(q.current_index(), Some(0));
    }

    #[test]
    fn test_repeat_list_full_cycle() {
        let mut q = Queue::new();
        q.push(test_track("A")); // 0
        q.push(test_track("B")); // 1
        q.push(test_track("C")); // 2
        q.cycle_play_mode(); // Sequential -> RepeatList

        // Advance to last track
        q.jump_to(2);
        // Advance past the end wraps to 0
        let t = q.advance().unwrap();
        assert_eq!(t.title, "A");
        assert_eq!(q.current_index(), Some(0));
        // Advance again goes to 1
        let t2 = q.advance().unwrap();
        assert_eq!(t2.title, "B");
        assert_eq!(q.current_index(), Some(1));
    }

    // ─── RepeatOne ────────────────────────────────────────────────────────────

    #[test]
    fn test_repeat_one_prev_stays() {
        let mut q = Queue::new();
        q.push(test_track("A"));
        q.push(test_track("B"));
        q.jump_to(1); // current = B
        q.cycle_play_mode(); // Sequential -> RepeatList
        q.cycle_play_mode(); // RepeatList -> RepeatOne

        // prev() also stays on the same track in RepeatOne mode
        let t = q.prev().unwrap();
        assert_eq!(t.title, "B");
        assert_eq!(q.current_index(), Some(1));
    }

    #[test]
    fn test_repeat_one_single_track() {
        let mut q = Queue::new();
        q.push(test_track("X"));
        q.cycle_play_mode(); // Sequential -> RepeatList
        q.cycle_play_mode(); // RepeatList -> RepeatOne

        // Both advance() and prev() return the only track
        assert_eq!(q.advance().unwrap().title, "X");
        assert_eq!(q.prev().unwrap().title, "X");
        assert_eq!(q.current_index(), Some(0));
    }

    // ─── Shuffle ──────────────────────────────────────────────────────────────

    #[test]
    fn test_shuffle_no_immediate_repeat_on_enter() {
        // With 4 tracks, entering Shuffle while on track B ensures the first
        // advance() never returns B.
        //
        // Proof: regenerate_shuffle_order() guarantees shuffle_order[0] != B.
        // cycle_play_mode() sets shuffle_pos = position_of(B) = p >= 1.
        // advance() increments to p+1:
        //   • if p+1 < 4: next = shuffle_order[p+1] which ≠ B (B is only at p)
        //   • if p+1 == 4: re-shuffle fires with current_index=B → new order[0]≠B
        let mut q = Queue::new();
        q.push(test_track("A")); // 0
        q.push(test_track("B")); // 1
        q.push(test_track("C")); // 2
        q.push(test_track("D")); // 3
        q.jump_to(1); // current = B
        q.cycle_play_mode(); // -> RepeatList
        q.cycle_play_mode(); // -> RepeatOne
        q.cycle_play_mode(); // -> Shuffle

        let next = q.advance().unwrap();
        assert_ne!(next.title, "B",
            "First advance() after entering Shuffle must not repeat the entry track");
    }

    #[test]
    fn test_shuffle_prev_stays_at_shuffle_pos_zero() {
        let mut q = Queue::new();
        for name in ["A", "B", "C", "D", "E"] {
            q.push(test_track(name));
        }
        q.cycle_play_mode(); // -> RepeatList
        q.cycle_play_mode(); // -> RepeatOne
        q.cycle_play_mode(); // -> Shuffle

        // Call prev() enough times to reach the floor (shuffle_pos = 0)
        for _ in 0..10 {
            q.prev();
        }
        assert_eq!(q.shuffle_pos, 0, "After many prev() calls shuffle_pos must be 0");

        let floor_title = q.current_track().unwrap().title.clone();
        q.prev();
        q.prev();
        assert_eq!(q.shuffle_pos, 0, "shuffle_pos must stay at 0 — no underflow");
        assert_eq!(q.current_track().unwrap().title, floor_title,
            "Track must remain the same when already at shuffle_pos 0");
    }

    #[test]
    fn test_shuffle_prev_navigates_backward() {
        // Round-trip test (deterministic regardless of random order):
        //   entering shuffle sets shuffle_pos = p >= 1
        //   prev() goes to p-1  (different track — it's a permutation)
        //   advance() goes back to p  (no reshuffle possible: p < len)
        let mut q = Queue::new();
        for name in ["A", "B", "C", "D", "E"] {
            q.push(test_track(name));
        }
        q.cycle_play_mode(); // -> RepeatList
        q.cycle_play_mode(); // -> RepeatOne
        q.cycle_play_mode(); // -> Shuffle

        let before_title = q.current_track().unwrap().title.clone();
        // shuffle_pos = p >= 1, so prev() moves to p-1
        let prev_title = q.prev().unwrap().title.clone();
        assert_ne!(prev_title, before_title,
            "prev() should move to a different track (p-1 ≠ p in a permutation)");

        // advance() from p-1 goes back to p — no reshuffle because p < len
        let fwd_title = q.advance().unwrap().title.clone();
        assert_eq!(fwd_title, before_title,
            "advance() after prev() must return to the original shuffle position");
    }

    #[test]
    fn test_shuffle_reshuffles_on_exhaustion() {
        // Advancing 2×n times forces at least one full reshuffle with n tracks.
        // After reshuffling, shuffle_order must still be a valid permutation and
        // every advance() must return a valid track.
        let mut q = Queue::new();
        q.push(test_track("A"));
        q.push(test_track("B"));
        q.push(test_track("C"));
        q.cycle_play_mode(); // -> RepeatList
        q.cycle_play_mode(); // -> RepeatOne
        q.cycle_play_mode(); // -> Shuffle

        let n = q.len();
        for step in 0..(n * 2) {
            let t = q.advance();
            assert!(t.is_some(), "advance() must always return a track in Shuffle (step {step})");
            let idx = q.current_index().unwrap();
            assert!(idx < n, "current_index {idx} must be < {n}");

            // Invariant: shuffle_order is always a valid permutation
            let mut order = q.shuffle_order.clone();
            order.sort_unstable();
            assert_eq!(order, (0..n).collect::<Vec<_>>(),
                "shuffle_order must be a permutation of 0..n after step {step}");
        }
    }

    #[test]
    fn test_shuffle_push_during_shuffle() {
        let mut q = Queue::new();
        q.push(test_track("A")); // 0
        q.push(test_track("B")); // 1
        q.push(test_track("C")); // 2
        q.cycle_play_mode(); // -> RepeatList
        q.cycle_play_mode(); // -> RepeatOne
        q.cycle_play_mode(); // -> Shuffle

        // push() in Shuffle mode inserts the new index after current shuffle_pos
        let old_shuffle_len = q.shuffle_order.len();
        q.push(test_track("D")); // new index = 3
        assert_eq!(q.tracks.len(), old_shuffle_len + 1);
        assert_eq!(q.shuffle_order.len(), q.tracks.len(),
            "shuffle_order must stay in sync with tracks after push in Shuffle mode");
        // The new index must appear in shuffle_order exactly once
        let occurrences = q.shuffle_order.iter().filter(|&&i| i == 3).count();
        assert_eq!(occurrences, 1,
            "New track index must appear exactly once in shuffle_order");
    }

    #[test]
    fn test_restore_shuffle_mode() {
        // Queue::restore with PlayMode::Shuffle must pre-generate shuffle_order
        // and set a valid shuffle_pos.
        let tracks = vec![
            test_track("A"),
            test_track("B"),
            test_track("C"),
            test_track("D"),
        ];
        let n = tracks.len();
        let q = Queue::restore(tracks, Some(1), PlayMode::Shuffle);

        assert_eq!(q.play_mode, PlayMode::Shuffle);
        assert_eq!(q.shuffle_order.len(), n,
            "restore() must pre-generate shuffle_order for Shuffle mode");
        assert!(q.shuffle_pos < n,
            "shuffle_pos {} must be a valid index into shuffle_order", q.shuffle_pos);

        // shuffle_order must be a permutation of 0..n
        let mut order = q.shuffle_order.clone();
        order.sort_unstable();
        assert_eq!(order, (0..n).collect::<Vec<_>>(),
            "shuffle_order must be a permutation of all track indices after restore");
    }

    // ─── Mode switching ───────────────────────────────────────────────────────

    #[test]
    fn test_repeat_one_then_switch_to_sequential() {
        let mut q = Queue::new();
        q.push(test_track("A")); // 0
        q.push(test_track("B")); // 1
        q.push(test_track("C")); // 2
        q.jump_to(0); // current = A
        q.cycle_play_mode(); // -> RepeatList
        q.cycle_play_mode(); // -> RepeatOne

        // In RepeatOne, advance stays on A
        assert_eq!(q.advance().unwrap().title, "A");
        assert_eq!(q.advance().unwrap().title, "A");

        // Switch through Shuffle → Sequential
        q.cycle_play_mode(); // -> Shuffle
        q.cycle_play_mode(); // -> Sequential
        // current_index is preserved through mode switches (still A or wherever
        // the Shuffle initialiser placed it — but we forced jump_to(0) before
        // RepeatOne so current_index must still be 0 right after entering Shuffle)
        // After Shuffle→Sequential, current_index remains what Shuffle set it to.
        // We simply check that advance() now moves forward linearly.
        let ci = q.current_index().unwrap();
        if ci + 1 < q.len() {
            let expected_title = q.tracks()[ci + 1].title.clone();
            let nxt_title = q.advance().unwrap().title.clone();
            assert_eq!(q.current_index(), Some(ci + 1),
                "Sequential advance must increment current_index by 1");
            assert_eq!(nxt_title, expected_title);
        } else {
            // At the last track — Sequential advance stops
            assert!(q.advance().is_none());
        }
    }

    #[test]
    fn test_shuffle_then_switch_to_sequential() {
        let mut q = Queue::new();
        q.push(test_track("A")); // 0
        q.push(test_track("B")); // 1
        q.push(test_track("C")); // 2
        q.push(test_track("D")); // 3
        q.push(test_track("E")); // 4
        q.jump_to(0); // start at A

        q.cycle_play_mode(); // -> RepeatList
        q.cycle_play_mode(); // -> RepeatOne
        q.cycle_play_mode(); // -> Shuffle

        // Advance once in Shuffle — current moves to a different track
        q.advance();
        let idx_after_shuffle = q.current_index().unwrap();

        // Switch back to Sequential
        q.cycle_play_mode(); // Shuffle -> Sequential

        // current_index must be preserved across the mode switch
        assert_eq!(q.current_index(), Some(idx_after_shuffle),
            "Switching modes must not change current_index");

        // Sequential advance now moves forward by exactly 1
        let n = q.len();
        if idx_after_shuffle + 1 < n {
            let nxt = q.advance();
            assert!(nxt.is_some());
            assert_eq!(q.current_index(), Some(idx_after_shuffle + 1),
                "Sequential advance after shuffle must increment current_index by 1");
        } else {
            // At the last track — Sequential advance returns None
            assert!(q.advance().is_none(),
                "Sequential advance at last track must return None");
        }
    }

    // ─── Sequential (gap-filling) ─────────────────────────────────────────────

    #[test]
    fn test_sequential_prev_at_start_stays() {
        let mut q = Queue::new();
        q.push(test_track("A")); // 0
        q.push(test_track("B")); // 1
        q.push(test_track("C")); // 2
        q.jump_to(0);

        // Sequential: prev at index 0 must stay at 0
        let t = q.prev().unwrap();
        assert_eq!(t.title, "A");
        assert_eq!(q.current_index(), Some(0));
        // A second prev() must also stay
        let t2 = q.prev().unwrap();
        assert_eq!(t2.title, "A");
        assert_eq!(q.current_index(), Some(0));
    }
}
