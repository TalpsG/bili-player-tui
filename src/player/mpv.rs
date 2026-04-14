use std::time::Duration;

use async_trait::async_trait;
use libmpv2::Mpv;

use crate::error::AudioError;
use crate::player::AudioBackend;
use crate::queue::track::TrackSource;

/// mpv-based audio backend using libmpv2.
///
/// For P0, this is a simple synchronous implementation.
/// Event threading and async integration will be added in P1.
pub struct MpvBackend {
    pub mpv: Mpv,
}

impl MpvBackend {
    pub fn new() -> Result<Self, AudioError> {
        let mpv = Mpv::new().map_err(|e| AudioError::Mpv(format!("Failed to create mpv: {e}")))?;

        // Configure for audio-only playback
        mpv.set_property("vid", "no")
            .map_err(|e| AudioError::Mpv(format!("Failed to set vid=no: {e}")))?;
        mpv.set_property("force-seekable", true)
            .map_err(|e| AudioError::Mpv(format!("Failed to set force-seekable: {e}")))?;

        Ok(Self { mpv })
    }
}

#[async_trait]
impl AudioBackend for MpvBackend {
    async fn play(&mut self, source: &TrackSource) -> Result<(), AudioError> {
        // Set Referer header for anti-hotlink bypass
        self.mpv
            .set_property("referrer", source.referer.as_str())
            .map_err(|e| AudioError::Mpv(format!("Failed to set referrer: {e}")))?;

        // Load the audio stream
        self.mpv
            .command("loadfile", &[source.stream_url.as_str()])
            .map_err(|e| AudioError::Mpv(format!("Failed to load file: {e}")))?;

        Ok(())
    }

    fn pause(&mut self) -> Result<(), AudioError> {
        self.mpv
            .set_property("pause", true)
            .map_err(|e| AudioError::Mpv(format!("Failed to pause: {e}")))
    }

    fn resume(&mut self) -> Result<(), AudioError> {
        self.mpv
            .set_property("pause", false)
            .map_err(|e| AudioError::Mpv(format!("Failed to resume: {e}")))
    }

    fn stop(&mut self) -> Result<(), AudioError> {
        self.mpv
            .command("stop", &[] as &[&str])
            .map_err(|e| AudioError::Mpv(format!("Failed to stop: {e}")))
    }

    fn is_playing(&self) -> bool {
        self.mpv
            .get_property::<bool>("pause")
            .map(|p| !p)
            .unwrap_or(false)
    }

    fn seek(&mut self, offset: Duration) -> Result<(), AudioError> {
        let secs = offset.as_secs_f64();
        self.mpv
            .command("seek", &[secs.to_string().as_str()])
            .map_err(|e| AudioError::Mpv(format!("Failed to seek: {e}")))
    }

    fn seek_to(&mut self, position: Duration) -> Result<(), AudioError> {
        let secs = position.as_secs_f64();
        self.mpv
            .set_property("time-pos", secs)
            .map_err(|e| AudioError::Mpv(format!("Failed to seek to position: {e}")))
    }

    fn position(&self) -> Option<Duration> {
        self.mpv
            .get_property::<f64>("time-pos")
            .ok()
            .filter(|&t| t >= 0.0)
            .map(Duration::from_secs_f64)
    }

    fn duration(&self) -> Option<Duration> {
        self.mpv
            .get_property::<f64>("duration")
            .ok()
            .filter(|&d| d > 0.0)
            .map(Duration::from_secs_f64)
    }

    fn set_volume(&mut self, volume: u16) -> u16 {
        let clamped = volume.min(100);
        let _ = self.mpv.set_property("volume", clamped as f64);
        clamped
    }

    fn volume(&self) -> u16 {
        self.mpv
            .get_property::<f64>("volume")
            .ok()
            .map(|v| v as u16)
            .unwrap_or(100)
    }
}

/// Wait until playback ends. Used for CLI `play` command in P0.
pub fn wait_until_end(mpv: &mut Mpv) {
    loop {
        match mpv.wait_event(1.0) {
            Some(Ok(libmpv2::events::Event::EndFile(reason))) => {
                // EOF = 0 means normal end
                if reason == 0 {
                    break;
                }
            }
            Some(Ok(libmpv2::events::Event::Shutdown)) => break,
            _ => {}
        }
    }
}
