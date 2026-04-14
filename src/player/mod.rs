use std::time::Duration;

use async_trait::async_trait;

use crate::error::AudioError;
use crate::queue::track::TrackSource;

/// Audio playback backend trait.
///
/// Abstracts the audio player implementation. Currently only libmpv2,
/// but the trait allows future backends.
#[async_trait]
pub trait AudioBackend: Send + Sync {
    /// Begin playing the given track source. Stops current if any.
    async fn play(&mut self, source: &TrackSource) -> Result<(), AudioError>;

    /// Pause playback.
    fn pause(&mut self) -> Result<(), AudioError>;

    /// Resume playback.
    fn resume(&mut self) -> Result<(), AudioError>;

    /// Stop playback and release resources.
    fn stop(&mut self) -> Result<(), AudioError>;

    /// Is audio currently playing?
    fn is_playing(&self) -> bool;

    /// Seek relative to current position.
    fn seek(&mut self, offset: Duration) -> Result<(), AudioError>;

    /// Seek to absolute position.
    fn seek_to(&mut self, position: Duration) -> Result<(), AudioError>;

    /// Current playback position.
    fn position(&self) -> Option<Duration>;

    /// Total duration of current track.
    fn duration(&self) -> Option<Duration>;

    /// Set volume (0-100). Returns the new volume (may be clamped).
    fn set_volume(&mut self, volume: u16) -> u16;

    /// Get current volume (0-100).
    fn volume(&self) -> u16;
}

pub mod mpv;
pub mod normalize;
