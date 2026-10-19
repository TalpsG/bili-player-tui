use crate::queue::track::Track;

/// Events processed in the App main loop.
pub enum Event {
    /// crossterm terminal key event
    Key(crossterm::event::KeyEvent),
    /// 250ms tick for UI refresh
    Tick,
    /// Event from the mpv background thread
    Player(PlayerEvent),
    /// Terminal resize
    Resize(u16, u16),
    /// Async search results
    SearchResult(Vec<Track>),
}

/// Why mpv unloaded a file (`mpv_end_file_reason`).
///
/// The numeric values are deliberately hidden behind this enum: they are not
/// contiguous, and STOP in particular is easy to mistake for a failure.  mpv
/// emits STOP whenever *we* replace the file, which is what happens on every
/// manual track change -- reporting that as a playback error used to blank out
/// `is_playing` after the next track had already started.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EndReason {
    /// The track played all the way to the end.
    Eof,
    /// We loaded something else, or stopped playback.  Not a failure.
    Stop,
    /// mpv is shutting down.
    Quit,
    /// The file could not be played.
    Error,
    /// The file turned out to be a playlist; playback continues elsewhere.
    Redirect,
    /// A reason this build of mpv does not know about.
    Unknown(u32),
}

impl From<u32> for EndReason {
    fn from(reason: u32) -> Self {
        // Values from `mpv_end_file_reason`: EOF=0, STOP=2, QUIT=3, ERROR=4,
        // REDIRECT=5.  They are not contiguous, so match rather than offset.
        match reason {
            0 => Self::Eof,
            2 => Self::Stop,
            3 => Self::Quit,
            4 => Self::Error,
            5 => Self::Redirect,
            other => Self::Unknown(other),
        }
    }
}

/// Events from the mpv backend, sent via mpsc channel.
#[derive(Debug)]
pub enum PlayerEvent {
    /// mpv unloaded the current file, for the given reason.
    TrackEnded { generation: u64, reason: EndReason },
    /// A command was accepted by the channel but rejected by mpv.
    CommandFailed {
        generation: u64,
        operation: &'static str,
        message: String,
    },
    /// A seek operation completed.
    SeekCompleted,
    /// mpv is shutting down.
    Shutdown,
}

#[cfg(test)]
mod tests {
    use super::EndReason;

    #[test]
    fn maps_mpv_end_file_reasons() {
        assert_eq!(EndReason::from(0), EndReason::Eof);
        assert_eq!(EndReason::from(2), EndReason::Stop);
        assert_eq!(EndReason::from(3), EndReason::Quit);
        assert_eq!(EndReason::from(4), EndReason::Error);
        assert_eq!(EndReason::from(5), EndReason::Redirect);
        assert_eq!(EndReason::from(99), EndReason::Unknown(99));
    }

    #[test]
    fn stop_is_not_confused_with_error() {
        // Regression: switching tracks emits STOP, and treating it as a failure
        // blanked out `is_playing` over audio that was actually playing.
        assert_ne!(EndReason::from(2), EndReason::Error);
    }
}
