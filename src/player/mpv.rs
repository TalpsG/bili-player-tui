use std::sync::{Arc, Mutex};
use std::time::Duration;

use libmpv2::Mpv;
use tokio::sync::mpsc;
use tracing::warn;

use crate::error::AudioError;
use crate::event::{EndReason, PlayerEvent};
use crate::queue::track::TrackSource;

/// Commands sent from the main thread to the mpv event thread.
enum MpvCommand {
    LoadFile {
        generation: u64,
        url: String,
        referer: String,
    },
    Pause,
    Resume,
    Stop,
    SeekRelative(f64),
    SeekAbsolute(f64),
    SetVolume(f64),
    Shutdown,
}

/// Cached playback state, updated by the event thread.
#[derive(Debug, Default)]
struct PlaybackState {
    position: Option<Duration>,
    duration: Option<Duration>,
    playing: bool,
    volume: u16,
}

/// mpv-based audio backend with event threading.
///
/// The Mpv instance lives in a dedicated std::thread that polls events
/// and processes commands from a channel. The main thread sends commands
/// and reads cached state.
pub struct MpvBackend {
    cmd_tx: std::sync::mpsc::Sender<MpvCommand>,
    event_rx: mpsc::UnboundedReceiver<PlayerEvent>,
    state: Arc<Mutex<PlaybackState>>,
    generation: u64,
    thread_handle: Option<std::thread::JoinHandle<()>>,
}

impl MpvBackend {
    pub fn new() -> Result<Self, AudioError> {
        let mpv = Mpv::with_initializer(|init| {
            // Keep real backend tests isolated from the system audio device.
            #[cfg(test)]
            init.set_option("ao", "null")?;
            let _ = init;
            Ok(())
        })
        .map_err(|e| AudioError::Mpv(format!("Failed to create mpv: {e}")))?;

        mpv.set_property("vid", "no")
            .map_err(|e| AudioError::Mpv(format!("Failed to set vid=no: {e}")))?;
        mpv.set_property("force-seekable", true)
            .map_err(|e| AudioError::Mpv(format!("Failed to set force-seekable: {e}")))?;

        let (cmd_tx, cmd_rx) = std::sync::mpsc::channel();
        let (event_tx, event_rx) = mpsc::unbounded_channel();
        let state = Arc::new(Mutex::new(PlaybackState {
            volume: 100,
            playing: false,
            ..Default::default()
        }));

        // Spawn the mpv event thread
        let state_clone = state.clone();
        let handle = std::thread::spawn(move || {
            mpv_event_loop(mpv, cmd_rx, event_tx, state_clone);
        });

        Ok(Self {
            cmd_tx,
            event_rx,
            state,
            generation: 0,
            thread_handle: Some(handle),
        })
    }

    /// Get the event receiver for the main loop.
    pub fn event_rx(&mut self) -> &mut mpsc::UnboundedReceiver<PlayerEvent> {
        &mut self.event_rx
    }

    fn send_cmd(&self, cmd: MpvCommand) -> Result<(), AudioError> {
        self.cmd_tx
            .send(cmd)
            .map_err(|e| AudioError::Mpv(format!("mpv command channel closed: {e}")))
    }

    pub fn play(&mut self, source: &TrackSource) -> Result<(), AudioError> {
        self.generation += 1;
        self.send_cmd(MpvCommand::LoadFile {
            generation: self.generation,
            url: source.stream_url.clone(),
            referer: source.referer.clone(),
        })
    }

    pub fn generation(&self) -> u64 {
        self.generation
    }

    pub fn pause(&mut self) -> Result<(), AudioError> {
        self.send_cmd(MpvCommand::Pause)
    }

    pub fn resume(&mut self) -> Result<(), AudioError> {
        self.send_cmd(MpvCommand::Resume)
    }

    pub fn stop(&mut self) -> Result<(), AudioError> {
        self.send_cmd(MpvCommand::Stop)
    }

    pub fn is_playing(&self) -> bool {
        self.state.lock().unwrap().playing
    }

    pub fn seek(&mut self, offset_secs: f64) -> Result<(), AudioError> {
        self.send_cmd(MpvCommand::SeekRelative(offset_secs))
    }

    pub fn seek_to(&mut self, position: Duration) -> Result<(), AudioError> {
        self.send_cmd(MpvCommand::SeekAbsolute(position.as_secs_f64()))
    }

    pub fn position(&self) -> Option<Duration> {
        self.state.lock().unwrap().position
    }

    pub fn duration(&self) -> Option<Duration> {
        self.state.lock().unwrap().duration
    }

    pub fn set_volume(&mut self, volume: u16) -> u16 {
        let clamped = volume.min(100);
        let _ = self.send_cmd(MpvCommand::SetVolume(clamped as f64));
        self.state.lock().unwrap().volume = clamped;
        clamped
    }

    pub fn volume(&self) -> u16 {
        self.state.lock().unwrap().volume
    }

    /// Shutdown the mpv event thread. Called on app exit.
    pub fn shutdown(&mut self) -> Result<(), AudioError> {
        let _ = self.send_cmd(MpvCommand::Shutdown);
        if let Some(handle) = self.thread_handle.take() {
            let _ = handle.join();
        }
        Ok(())
    }
}

impl Drop for MpvBackend {
    fn drop(&mut self) {
        let _ = self.shutdown();
    }
}

/// The mpv event thread loop.
///
/// Owns the Mpv instance exclusively. Processes commands from the channel
/// and polls mpv events, forwarding relevant ones to the app.
fn mpv_event_loop(
    mut mpv: Mpv,
    cmd_rx: std::sync::mpsc::Receiver<MpvCommand>,
    event_tx: mpsc::UnboundedSender<PlayerEvent>,
    state: Arc<Mutex<PlaybackState>>,
) {
    let mut active_generation = 0;
    let mut starting_generation = None;
    loop {
        // Drain all pending commands (non-blocking)
        // Await START_FILE before accepting another load. This gives each
        // END_FILE a stable request identity even when commands arrive rapidly.
        while starting_generation.is_none() {
            let Ok(cmd) = cmd_rx.try_recv() else { break };
            match cmd {
                MpvCommand::LoadFile {
                    generation,
                    url,
                    referer,
                } => {
                    let result = mpv
                        .set_property("referrer", referer.as_str())
                        .and_then(|_| mpv.command("loadfile", &[url.as_str()]));
                    if result.is_ok() {
                        starting_generation = Some(generation);
                    }
                    let result = result
                        // pause is persistent across loadfile: explicitly selecting
                        // another track should start it even if the old one paused.
                        .and_then(|_| mpv.set_property("pause", false));
                    if let Err(error) = result {
                        let _ = event_tx.send(PlayerEvent::CommandFailed {
                            generation,
                            operation: "loadfile",
                            message: error.to_string(),
                        });
                    }
                }
                MpvCommand::Pause => {
                    let _ = mpv.set_property("pause", true);
                }
                MpvCommand::Resume => {
                    let _ = mpv.set_property("pause", false);
                }
                MpvCommand::Stop => {
                    let _ = mpv.command("stop", &[] as &[&str]);
                }
                MpvCommand::SeekRelative(secs) => {
                    let _ = mpv.command("seek", &[secs.to_string().as_str()]);
                }
                MpvCommand::SeekAbsolute(secs) => {
                    let _ = mpv.set_property("time-pos", secs);
                }
                MpvCommand::SetVolume(vol) => {
                    let _ = mpv.set_property("volume", vol);
                }
                MpvCommand::Shutdown => {
                    let _ = mpv.command("quit", &[] as &[&str]);
                    return;
                }
            }
        }

        // Update cached state
        {
            let mut s = state.lock().unwrap();
            s.position = mpv
                .get_property::<f64>("time-pos")
                .ok()
                .filter(|&t| t >= 0.0)
                .map(Duration::from_secs_f64);
            s.duration = mpv
                .get_property::<f64>("duration")
                .ok()
                .filter(|&d| d > 0.0)
                .map(Duration::from_secs_f64);
            let idle = mpv.get_property::<bool>("idle-active").unwrap_or(true);
            s.playing = !idle
                && mpv
                    .get_property::<bool>("pause")
                    .map(|p| !p)
                    .unwrap_or(false);
            s.volume = mpv
                .get_property::<f64>("volume")
                .ok()
                .map(|v| v as u16)
                .unwrap_or(100);
        }

        // Wait for mpv event (short timeout to keep command processing responsive)
        match mpv.wait_event(0.1) {
            Some(Ok(libmpv2::events::Event::StartFile)) => {
                if let Some(generation) = starting_generation.take() {
                    active_generation = generation;
                }
            }
            Some(Ok(libmpv2::events::Event::EndFile(reason))) => {
                let _ = event_tx.send(PlayerEvent::TrackEnded {
                    generation: active_generation,
                    reason: EndReason::from(reason),
                });
            }
            Some(Ok(libmpv2::events::Event::PlaybackRestart)) => {
                let _ = event_tx.send(PlayerEvent::SeekCompleted);
            }
            Some(Ok(libmpv2::events::Event::Shutdown)) => {
                let _ = event_tx.send(PlayerEvent::Shutdown);
                return;
            }
            Some(Err(e)) => {
                // libmpv2 turns END_FILE with a negative file error into Err,
                // discarding the reason. No async commands or property observers
                // are used here, so propagate it as a playback failure instead
                // of silently losing the stream's only failure notification.
                warn!("mpv event error: {e}");
                let _ = event_tx.send(PlayerEvent::CommandFailed {
                    generation: active_generation,
                    operation: "playback",
                    message: e.to_string(),
                });
            }
            _ => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::queue::track::AudioQuality;

    fn short_audio(dir: &std::path::Path) -> TrackSource {
        // One second of silence, PCM mono 8 kHz, generated locally.
        let bytes = 16000_u32;
        let mut wav = Vec::new();
        wav.extend_from_slice(b"RIFF");
        wav.extend_from_slice(&(36 + bytes).to_le_bytes());
        wav.extend_from_slice(b"WAVEfmt ");
        wav.extend_from_slice(&16_u32.to_le_bytes());
        wav.extend_from_slice(&1_u16.to_le_bytes());
        wav.extend_from_slice(&1_u16.to_le_bytes());
        wav.extend_from_slice(&8000_u32.to_le_bytes());
        wav.extend_from_slice(&16000_u32.to_le_bytes());
        wav.extend_from_slice(&2_u16.to_le_bytes());
        wav.extend_from_slice(&16_u16.to_le_bytes());
        wav.extend_from_slice(b"data");
        wav.extend_from_slice(&bytes.to_le_bytes());
        wav.resize(44 + bytes as usize, 0);
        let path = dir.join("short.wav");
        std::fs::write(&path, wav).unwrap();
        TrackSource {
            stream_url: path.to_string_lossy().into_owned(),
            referer: String::new(),
            audio_quality: AudioQuality::LegacyMp4,
        }
    }

    async fn wait_playing(backend: &MpvBackend, expected: bool) {
        tokio::time::timeout(Duration::from_secs(5), async {
            while backend.is_playing() != expected {
                tokio::time::sleep(Duration::from_millis(20)).await;
            }
        })
        .await
        .expect("mpv playback state did not settle");
    }

    async fn wait_eof(backend: &mut MpvBackend) {
        tokio::time::timeout(Duration::from_secs(5), async {
            loop {
                match backend.event_rx().recv().await.unwrap() {
                    PlayerEvent::TrackEnded {
                        reason: EndReason::Eof,
                        ..
                    } => break,
                    PlayerEvent::CommandFailed {
                        operation, message, ..
                    } => panic!("{operation}: {message}"),
                    _ => {}
                }
            }
        })
        .await
        .expect("missing actual mpv EOF");
    }

    #[tokio::test]
    async fn real_mpv_local_audio_advances_queue_and_stops_at_tail() {
        use crate::queue::{Queue, track::Track};
        let dir = tempfile::tempdir().unwrap();
        let source = short_audio(dir.path());
        let tracks = (0..3)
            .map(|i| Track {
                bvid: format!("local{i}"),
                cid: i,
                title: format!("local{i}"),
                author: String::new(),
                duration: Duration::from_secs(1),
                cover_url: None,
                part: None,
                page: None,
                source: Some(source.clone()),
            })
            .collect();
        let mut queue = Queue::restore(tracks, Some(0), crate::queue::PlayMode::Sequential);
        let mut backend = MpvBackend::new().unwrap();
        assert!(!backend.is_playing());
        for i in 0..3 {
            assert_eq!(queue.current_index(), Some(i));
            backend
                .play(queue.current_track().unwrap().source.as_ref().unwrap())
                .unwrap();
            wait_playing(&backend, true).await;
            wait_eof(&mut backend).await;
            wait_playing(&backend, false).await;
            queue.advance();
        }
        assert_eq!(queue.current_index(), None);
        assert!(
            !backend.is_playing(),
            "idle mpv must not look like playback"
        );
    }

    #[tokio::test]
    async fn real_mpv_replacement_events_keep_their_load_generation() {
        let dir = tempfile::tempdir().unwrap();
        let source = short_audio(dir.path());
        let mut backend = MpvBackend::new().unwrap();
        backend.play(&source).unwrap();
        wait_playing(&backend, true).await;
        backend.play(&source).unwrap();
        backend.play(&source).unwrap();
        let latest = backend.generation();
        let mut stopped = Vec::new();
        let ended = tokio::time::timeout(Duration::from_secs(8), async {
            loop {
                match backend.event_rx().recv().await.unwrap() {
                    PlayerEvent::TrackEnded {
                        generation,
                        reason: EndReason::Stop,
                    } => stopped.push(generation),
                    PlayerEvent::TrackEnded {
                        generation,
                        reason: EndReason::Eof,
                    } => break generation,
                    PlayerEvent::CommandFailed {
                        operation, message, ..
                    } => panic!("{operation}: {message}"),
                    _ => {}
                }
            }
        })
        .await
        .unwrap();
        assert_eq!(stopped, vec![1, 2]);
        assert_eq!(ended, latest);
    }

    #[tokio::test]
    async fn real_mpv_selecting_after_pause_resumes_and_reports_command_errors() {
        let dir = tempfile::tempdir().unwrap();
        let source = short_audio(dir.path());
        let mut backend = MpvBackend::new().unwrap();
        backend.play(&source).unwrap();
        wait_playing(&backend, true).await;
        backend.pause().unwrap();
        wait_playing(&backend, false).await;
        backend.play(&source).unwrap();
        wait_playing(&backend, true).await;
        wait_eof(&mut backend).await;
        let mut invalid = source;
        invalid.stream_url = "invalid\0url".into();
        backend.play(&invalid).unwrap(); // Channel success is not command success.
        let event = tokio::time::timeout(Duration::from_secs(5), async {
            loop {
                if let Some(event @ PlayerEvent::CommandFailed { .. }) =
                    backend.event_rx().recv().await
                {
                    break event;
                }
            }
        })
        .await
        .unwrap();
        assert!(matches!(
            event,
            PlayerEvent::CommandFailed {
                operation: "loadfile",
                ..
            }
        ));
        // Rejection must not leave the START_FILE barrier stuck.
        backend.play(&short_audio(dir.path())).unwrap();
        wait_playing(&backend, true).await;
        wait_eof(&mut backend).await;
        let missing = TrackSource {
            stream_url: dir
                .path()
                .join("missing.wav")
                .to_string_lossy()
                .into_owned(),
            referer: String::new(),
            audio_quality: AudioQuality::LegacyMp4,
        };
        backend.play(&missing).unwrap();
        let operation = tokio::time::timeout(Duration::from_secs(5), async {
            loop {
                if let Some(PlayerEvent::CommandFailed {
                    generation,
                    operation,
                    ..
                }) = backend.event_rx().recv().await
                {
                    if generation == backend.generation() {
                        break operation;
                    }
                }
            }
        })
        .await
        .unwrap();
        assert_eq!(operation, "playback");
        wait_playing(&backend, false).await;
    }
}
