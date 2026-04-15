use std::sync::{Arc, Mutex};
use std::time::Duration;

use libmpv2::Mpv;
use tokio::sync::mpsc;
use tracing::warn;

use crate::error::AudioError;
use crate::event::PlayerEvent;
use crate::queue::track::TrackSource;

/// Commands sent from the main thread to the mpv event thread.
enum MpvCommand {
    LoadFile {
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
    thread_handle: Option<std::thread::JoinHandle<()>>,
}

impl MpvBackend {
    pub fn new() -> Result<Self, AudioError> {
        let mpv = Mpv::new().map_err(|e| AudioError::Mpv(format!("Failed to create mpv: {e}")))?;

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
        self.send_cmd(MpvCommand::LoadFile {
            url: source.stream_url.clone(),
            referer: source.referer.clone(),
        })
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
    loop {
        // Drain all pending commands (non-blocking)
        while let Ok(cmd) = cmd_rx.try_recv() {
            match cmd {
                MpvCommand::LoadFile { url, referer } => {
                    let _ = mpv.set_property("referrer", referer.as_str());
                    let _ = mpv.command("loadfile", &[url.as_str()]);
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
            s.playing = mpv
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
            Some(Ok(libmpv2::events::Event::EndFile(reason))) => {
                let _ = event_tx.send(PlayerEvent::TrackEnded { reason });
            }
            Some(Ok(libmpv2::events::Event::PlaybackRestart)) => {
                let _ = event_tx.send(PlayerEvent::SeekCompleted);
            }
            Some(Ok(libmpv2::events::Event::Shutdown)) => {
                let _ = event_tx.send(PlayerEvent::Shutdown);
                return;
            }
            Some(Err(e)) => {
                warn!("mpv event error: {e}");
            }
            _ => {}
        }
    }
}
