//! A single visible search row's animation; no queue or persistence state.
use std::time::{Duration, Instant};

use unicode_segmentation::UnicodeSegmentation;
use unicode_width::UnicodeWidthStr;

const HOLD_MS: u128 = 1_000;
const STEP_MS: u128 = 500;

#[derive(Default)]
pub struct Marquee {
    identity: String,
    width: usize,
    started: Option<Instant>,
}

impl Marquee {
    pub fn reset(&mut self) {
        self.identity.clear();
        self.started = None;
    }

    pub fn offset(&mut self, identity: &str, text: &str, width: usize, now: Instant) -> usize {
        let overflow = text.width().saturating_sub(width);
        if width == 0 || overflow == 0 {
            self.reset();
            return 0;
        }
        if self.started.is_none() || self.identity != identity || self.width != width {
            self.identity.clear();
            self.identity.push_str(identity);
            self.width = width;
            self.started = Some(now);
        }
        position(
            now.saturating_duration_since(self.started.unwrap()),
            overflow,
        )
    }
}

fn position(elapsed: Duration, overflow: usize) -> usize {
    let travel = overflow as u128 * STEP_MS;
    let time = elapsed.as_millis() % (2 * HOLD_MS + 2 * travel);
    if time < HOLD_MS {
        0
    } else if time < HOLD_MS + travel {
        ((time - HOLD_MS) / STEP_MS) as usize
    } else if time < 2 * HOLD_MS + travel {
        overflow
    } else {
        overflow - ((time - 2 * HOLD_MS - travel) / STEP_MS) as usize
    }
}

/// Crop by terminal columns, preserving complete extended grapheme clusters.
/// A partially visible wide cluster becomes blank columns at either edge.
pub fn window(text: &str, offset: usize, width: usize) -> String {
    let end = offset.saturating_add(width);
    let mut column = 0usize;
    let mut result = String::new();
    for grapheme in text.graphemes(true) {
        let next = column.saturating_add(grapheme.width());
        if next > offset && column < end {
            if column >= offset && next <= end {
                result.push_str(grapheme);
            } else {
                result.push_str(&" ".repeat(next.min(end).saturating_sub(column.max(offset))));
            }
        }
        column = next;
        if column >= end {
            break;
        }
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn holds_and_travels_in_both_directions() {
        let samples = [
            (0, 0),
            (999, 0),
            (1500, 1),
            (2000, 2),
            (2999, 2),
            (3000, 2),
            (3500, 1),
            (4000, 0),
            (4999, 0),
            (5500, 1),
        ];
        for (ms, expected) in samples {
            assert_eq!(position(Duration::from_millis(ms), 2), expected, "{ms}");
        }
    }

    #[test]
    fn resets_on_identity_width_and_explicit_reset() {
        let now = Instant::now();
        let later = now + Duration::from_millis(2000);
        let mut state = Marquee::default();
        assert_eq!(state.offset("one", "abcdef", 4, now), 0);
        assert_eq!(state.offset("one", "abcdef", 4, later), 2);
        assert_eq!(state.offset("two", "abcdef", 4, later), 0);
        assert_eq!(state.offset("two", "abcdef", 3, later), 0);
        state.reset();
        assert_eq!(state.offset("two", "abcdef", 3, later), 0);
        assert_eq!(state.offset("two", "abc", 3, later), 0);
        assert!(state.started.is_none());
        assert_eq!(state.offset("two", "abcdef", 0, later), 0);
        assert!(state.started.is_none());
        assert_eq!(state.offset("two", "abcdef", 1, later), 0);
        assert_eq!(
            state.offset("two", "abcdef", 1, later + Duration::from_millis(1500)),
            1
        );
    }

    #[test]
    fn windows_preserve_unicode_and_fit_zero_or_one_column() {
        let text = "中a\u{301}👩‍💻🇨🇳末";
        assert_eq!(window(text, 0, 2), "中");
        assert_eq!(window(text, 2, 1), "a\u{301}");
        assert_eq!(window(text, 3, 2), "👩‍💻");
        assert_eq!(window(text, 5, 2), "🇨🇳");
        assert_eq!(window(text, 1, 2), " a\u{301}");
        for width in 0..12 {
            for offset in 0..12 {
                assert!(window(text, offset, width).width() <= width);
            }
        }
        assert_eq!(window(text, 0, 0), "");
        assert_eq!(window(text, 0, 1), " ");
    }
}
