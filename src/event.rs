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

/// Events from the mpv backend, sent via mpsc channel.
#[derive(Debug)]
pub enum PlayerEvent {
    /// Current track ended. reason: 0=EOF, 2=stop, 4=error.
    TrackEnded { reason: u32 },
    /// A seek operation completed.
    SeekCompleted,
    /// mpv is shutting down.
    Shutdown,
}
