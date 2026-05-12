use std::time::Duration;

use crossterm::{
    event::{EnableFocusChange, DisableFocusChange, Event as CrosstermEvent, KeyCode, KeyEvent},
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use ratatui::{backend::CrosstermBackend, Terminal};
use tokio::sync::mpsc;

use crate::bilibili::api::BilibiliClient;
use crate::bilibili::search::search_videos;
use crate::bilibili::stream::get_audio_stream;
use crate::bilibili::video::get_video_info;
use crate::command::Command;
use crate::config::Config;
use crate::cover::CoverManager;
use crate::event::PlayerEvent;
use crate::player::mpv::MpvBackend;
use crate::playlist::Playlist;
use crate::playlist::storage::PlaylistStore;
use crate::queue::Queue;
use crate::queue::track::Track;
use crate::state::AppState;
use crate::ui::Ui;

/// Input mode for key handling.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InputMode {
    Normal,
    SearchInput,
    SearchNormal,
}

/// Which column has keyboard focus.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FocusColumn {
    Playlist,
    TrackList,
    Detail,
}

impl FocusColumn {
    /// All columns in Tab order.
    const ORDER: [FocusColumn; 3] = [
        FocusColumn::Playlist,
        FocusColumn::TrackList,
        FocusColumn::Detail,
    ];

    /// Cycle to next visible column.
    pub fn next(self, playlist_visible: bool, detail_visible: bool) -> Self {
        let visible: Vec<_> = Self::ORDER
            .iter()
            .filter(|col| match col {
                FocusColumn::Playlist => playlist_visible,
                FocusColumn::TrackList => true,
                FocusColumn::Detail => detail_visible,
            })
            .copied()
            .collect();
        let idx = visible.iter().position(|&c| c == self).unwrap_or(0);
        visible[(idx + 1) % visible.len()]
    }

    pub fn prev(self, playlist_visible: bool, detail_visible: bool) -> Self {
        let visible: Vec<_> = Self::ORDER
            .iter()
            .filter(|col| match col {
                FocusColumn::Playlist => playlist_visible,
                FocusColumn::TrackList => true,
                FocusColumn::Detail => detail_visible,
            })
            .copied()
            .collect();
        let idx = visible.iter().position(|&c| c == self).unwrap_or(0);
        visible[(idx + visible.len() - 1) % visible.len()]
    }
}

/// Popup overlay layers, in z-order.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PopupLayer {
    VolumeSlider,
    Search,
    Help,
    PlaylistCreate,        // input popup for playlist name
    PlaylistDeleteConfirm, // confirm popup before deletion
    AddToPlaylist,         // list-chooser popup for adding track to playlist
}

/// The App is the sole state owner.
/// All state flows through here; UI reads from App, commands mutate App.
pub struct App {
    // Config & API
    pub config: Config,
    pub client: BilibiliClient,

    // Player
    pub player: MpvBackend,

    // Queue
    pub queue: Queue,

    // Playlists
    /// User-created playlists. Index 0 absent here; playlist_cursor==0 maps to self.queue.
    pub playlists: Vec<Playlist>,
    /// Which row is selected in the left (playlist) column.
    /// 0 = Queue (virtual), 1..=N = playlists[0..N-1].
    pub playlist_cursor: usize,
    /// Buffer accumulating characters in the CreatePlaylist popup.
    pub playlist_name_input: String,
    /// Cursor in the add-to-playlist chooser popup (0=Queue, 1..N=playlists).
    pub add_to_playlist_cursor: usize,
    /// Track staged for "add to playlist" operation (set when AddToPlaylistOpen fires).
    pub add_to_playlist_track: Option<Track>,

    // UI state
    pub ui: Ui,
    pub input_mode: InputMode,
    pub focus_column: FocusColumn,

    // Search state
    pub search_query: String,
    pub search_query_cursor: usize,
    pub search_focus_input: bool,
    pub search_results: Vec<Track>,
    pub search_cursor: usize,
    pub searching: bool,

    // Player state cache
    pub is_playing: bool,
    pub position: Option<Duration>,
    pub duration: Option<Duration>,
    pub volume: u16,
    pub muted: bool,
    pub volume_before_mute: u16,

    // Popup state
    pub popup_stack: Vec<PopupLayer>,
    pub volume_popup_time: Option<std::time::Instant>,

    // Status message (shown in status bar for errors/notifications)
    pub status_message: Option<String>,

    // Login info
    pub logged_in: bool,

    // Shutdown flag
    should_quit: bool,

    // Terminal size
    pub terminal_width: u16,
    pub terminal_height: u16,

    // Async state receivers
    pub search_result_rx: Option<mpsc::UnboundedReceiver<Result<Vec<Track>, crate::error::BilibiliError>>>,
    pub pending_stream: Option<(Track, mpsc::UnboundedReceiver<Result<crate::queue::track::TrackSource, crate::error::BilibiliError>>)>,
    /// Pending BV fetch: resolves video info for a directly-entered BV ID / URL.
    pub pending_bv_fetch: Option<mpsc::UnboundedReceiver<Result<Track, crate::error::BilibiliError>>>,

    // Cover image manager (None if terminal doesn't support graphics)
    pub cover_manager: Option<CoverManager>,

    /// Set to `true` to force `terminal.clear()` before the next draw.
    /// Used by Ctrl+L (manual redraw) and automatic tmux focus-regain handling.
    pub force_redraw: bool,

    /// When `Some`, cover rendering is suppressed until this instant passes.
    /// Set on every cursor movement; expires 100 ms after the last move.
    /// This prevents the terminal from transmitting raw pixel data (Kitty/Sixel)
    /// on every j/k keypress, which causes noticeable input lag.
    pub cover_render_after: Option<std::time::Instant>,
}

/// Extract a BV ID from a raw query string.
///
/// Accepts:
/// - Bare BV ID: `BV1xx411c7mD`
/// - Full URL: `https://www.bilibili.com/video/BV1xx411c7mD`
/// - Short URL or query string containing `BV...`
///
/// Returns `None` if no BV ID pattern found.
fn extract_bvid(query: &str) -> Option<String> {
    // BV IDs are exactly "BV" followed by 10 alphanumeric characters.
    let q = query.trim();
    // Walk through the string looking for "BV" followed by 10 alnum chars.
    let bytes = q.as_bytes();
    let len = bytes.len();
    let mut i = 0;
    while i + 12 <= len {
        if bytes[i] == b'B' && bytes[i + 1] == b'V' {
            let tail = &bytes[i + 2..i + 12];
            if tail.iter().all(|b| b.is_ascii_alphanumeric()) {
                // Make sure the char after (if any) is not alphanumeric (word boundary)
                let after_ok = i + 12 >= len || !bytes[i + 12].is_ascii_alphanumeric();
                if after_ok {
                    return Some(q[i..i + 12].to_string());
                }
            }
        }
        i += 1;
    }
    None
}

impl App {
    pub fn new(config: Config, picker: Option<ratatui_image::picker::Picker>) -> anyhow::Result<Self> {
        let logged_in = !config.bilibili.sessdata.is_empty();
        let client = BilibiliClient::new(Some(config.bilibili.sessdata.clone()));
        let mut player = MpvBackend::new()?;
        let volume = config.player.volume;
        player.set_volume(volume);

        let mut app = Self {
            config,
            client,
            player,
            queue: Queue::new(),
            playlists: PlaylistStore::load()
                .map(|s| s.playlists)
                .unwrap_or_default(),
            playlist_cursor: 0,
            playlist_name_input: String::new(),
            add_to_playlist_cursor: 0,
            add_to_playlist_track: None,
            ui: Ui::new(),
            input_mode: InputMode::Normal,
            focus_column: FocusColumn::TrackList,
            search_query: String::new(),
            search_query_cursor: 0,
            search_focus_input: true,
            search_results: Vec::new(),
            search_cursor: 0,
            searching: false,
            is_playing: false,
            position: None,
            duration: None,
            volume,
            muted: false,
            volume_before_mute: volume,
            popup_stack: Vec::new(),
            volume_popup_time: None,
            status_message: None,
            logged_in,
            should_quit: false,
            terminal_width: 80,
            terminal_height: 24,
            search_result_rx: None,
            pending_stream: None,
            pending_bv_fetch: None,
            cover_manager: picker.map(CoverManager::new),
            force_redraw: false,
            cover_render_after: None,
        };

        // Prewarm cover cache for all known tracks so L1 is populated from disk
        // (or network) immediately on startup rather than waiting for first render.
        if let Some(ref mut mgr) = app.cover_manager {
            // Prewarm queue tracks
            for track in app.queue.tracks() {
                if let Some(url) = &track.cover_url {
                    mgr.prewarm(url);
                }
            }
            // Prewarm playlist tracks
            for pl in &app.playlists {
                for track in &pl.tracks {
                    if let Some(url) = &track.cover_url {
                        mgr.prewarm(url);
                    }
                }
            }
        }

        Ok(app)
    }

    /// Run the TUI main loop.
    pub async fn run(&mut self) -> anyhow::Result<()> {
        // Setup terminal
        enable_raw_mode()?;
        let mut stdout = std::io::stdout();
        execute!(stdout, EnterAlternateScreen, EnableFocusChange)?;
        let backend = CrosstermBackend::new(stdout);
        let mut terminal = Terminal::new(backend)?;
        terminal.clear()?;

        // Enable tmux focus-events so FocusGained is forwarded to us when the
        // user switches back to this tmux window.  This is the tmux side-channel;
        // crossterm's EnableFocusChange sends \033[?1004h to the terminal, but
        // inside tmux that only works if tmux itself has `focus-events on`.
        // We temporarily enable it and restore the old value on exit.
        let tmux_focus_was_on = enable_tmux_focus_events();

        // Set up panic hook to restore terminal
        let panic_hook = std::panic::take_hook();
        std::panic::set_hook(Box::new(move |info| {
            let _ = disable_raw_mode();
            let _ = execute!(std::io::stdout(), LeaveAlternateScreen, DisableFocusChange);
            panic_hook(info);
        }));

        // Event channels
        let (key_tx, mut key_rx) = mpsc::unbounded_channel::<CrosstermEvent>();

        // Spawn crossterm event reader task
        tokio::task::spawn_blocking(move || {
            loop {
                // Poll for events with a short timeout to allow checking if the channel is still alive
                if let Ok(true) = crossterm::event::poll(Duration::from_millis(500))
                    && let Ok(event) = crossterm::event::read()
                {
                    match &event {
                        CrosstermEvent::Key(_) | CrosstermEvent::FocusGained => {
                            if key_tx.send(event).is_err() {
                                break;
                            }
                        }
                        _ => {}
                    }
                }
                // If receiver dropped, exit loop
                if key_tx.is_closed() {
                    break;
                }
            }
        });

        // Tick interval (250ms for 4 FPS)
        let mut tick_interval = tokio::time::interval(Duration::from_millis(250));

        // Restore queue state from previous session
        match AppState::load() {
            Ok(state) if !state.queue_tracks.is_empty() => {
                self.queue = Queue::restore(
                    state.queue_tracks,
                    state.queue_current_index,
                    state.play_mode,
                );
                // Sync UI cursor to restored current index
                if let Some(idx) = self.queue.current_index() {
                    self.ui.track_list_cursor = idx;
                }
            }
            Ok(_) => {} // empty state, start fresh
            Err(e) => {
                // Non-fatal: just log and continue with an empty queue
                eprintln!("Warning: failed to load state: {e}");
            }
        }

        let result = self.run_loop(&mut terminal, &mut key_rx, &mut tick_interval).await;

        // Restore terminal
        disable_raw_mode()?;
        execute!(terminal.backend_mut(), LeaveAlternateScreen, DisableFocusChange)?;

        // Restore tmux focus-events to whatever it was before we started
        restore_tmux_focus_events(tmux_focus_was_on);

        let _ = self.player.shutdown();

        // Persist queue state on clean exit
        let (queue_tracks, queue_current_index, play_mode) = self.queue.snapshot();
        let app_state = AppState { queue_tracks, queue_current_index, play_mode };
        if let Err(e) = app_state.save() {
            eprintln!("Warning: failed to save state: {e}");
        }

        // Persist playlists on clean exit
        let store = PlaylistStore { playlists: self.playlists.clone() };
        if let Err(e) = store.save() {
            eprintln!("Warning: failed to save playlists: {e}");
        }

        result
    }

    async fn run_loop(
        &mut self,
        terminal: &mut Terminal<CrosstermBackend<std::io::Stdout>>,
        key_rx: &mut mpsc::UnboundedReceiver<CrosstermEvent>,
        tick_interval: &mut tokio::time::Interval,
    ) -> anyhow::Result<()> {
        loop {
            if self.should_quit {
                break;
            }

            tokio::select! {
                // Key / focus events from crossterm
                event = key_rx.recv() => {
                    match event {
                        Some(CrosstermEvent::Key(key)) => self.handle_key(key),
                        Some(CrosstermEvent::FocusGained) => {
                            // Terminal regained focus (e.g. tmux window switch back).
                            // 1. Rebuild cover protocol states so images are re-encoded.
                            // 2. Only call terminal.clear() for protocols whose pixel data
                            //    lives outside ratatui's cell buffer (Sixel, Kitty, iTerm2).
                            //    For Halfblocks, tmux preserves the Unicode half-block chars
                            //    in its own cell buffer — no clear needed, no flash.
                            let needs_clear = self.cover_manager
                                .as_ref()
                                .map(|mgr| mgr.needs_terminal_clear_on_focus())
                                .unwrap_or(false);
                            if let Some(mgr) = &mut self.cover_manager {
                                mgr.invalidate_all();
                            }
                            if needs_clear {
                                let _ = terminal.clear();
                            }
                        }
                        _ => {}
                    }
                }

                // Tick timer
                _ = tick_interval.tick() => {
                    self.handle_tick();
                }

                // Player events from mpv thread
                player_event = self.player.event_rx().recv() => {
                    if let Some(event) = player_event {
                        self.handle_player_event(event);
                    }
                }
            }

            // Update cached state from player
            self.is_playing = self.player.is_playing();
            self.position = self.player.position();
            self.duration = self.player.duration();

            // Render — clear first if a full re-draw was requested (e.g. after
            // tmux focus-regain or Ctrl+L) so ratatui re-emits all escape seqs.
            if self.force_redraw {
                self.force_redraw = false;
                let _ = terminal.clear();
            }
            terminal.draw(|f| self.draw(f))?;
        }

        Ok(())
    }

    fn handle_key(&mut self, key: KeyEvent) {
        // If VolumeSlider is the top popup, Esc should close it
        if self.popup_stack.last() == Some(&PopupLayer::VolumeSlider) && key.code == KeyCode::Esc {
            self.hide_popup(PopupLayer::VolumeSlider);
            return;
        }

        let cmd = crate::ui::controls::key_to_command(
            key,
            self.input_mode,
            self.focus_column,
            &self.popup_stack,
            self.search_results.len(),
        );
        if cmd != Command::Noop {
            self.handle_command(cmd);
        }
    }

    fn handle_command(&mut self, cmd: Command) {
        match cmd {
            Command::Quit => {
                self.should_quit = true;
            }
            Command::TogglePause => {
                if self.is_playing {
                    let _ = self.player.pause();
                } else {
                    let _ = self.player.resume();
                }
            }
            Command::NextTrack => {
                if let Some(track) = self.queue.advance() {
                    let track = track.clone();
                    self.play_track(track);
                }
            }
            Command::PrevTrack => {
                if let Some(track) = self.queue.prev() {
                    let track = track.clone();
                    self.play_track(track);
                }
            }
            Command::SeekForward => {
                let _ = self.player.seek(5.0);
            }
            Command::SeekBackward => {
                let _ = self.player.seek(-5.0);
            }
            Command::VolumeUp => {
                let new_vol = (self.volume + 5).min(100);
                self.volume = self.player.set_volume(new_vol);
                self.muted = false;
                self.show_popup(PopupLayer::VolumeSlider);
            }
            Command::VolumeDown => {
                let new_vol = self.volume.saturating_sub(5);
                self.volume = self.player.set_volume(new_vol);
                self.muted = false;
                self.show_popup(PopupLayer::VolumeSlider);
            }
            Command::ToggleMute => {
                if self.muted {
                    self.volume = self.player.set_volume(self.volume_before_mute);
                    self.muted = false;
                } else {
                    self.volume_before_mute = self.volume;
                    self.muted = true;
                    self.player.set_volume(0);
                }
                self.show_popup(PopupLayer::VolumeSlider);
            }
            Command::CyclePlayMode => {
                self.queue.cycle_play_mode();
                let label = match self.queue.play_mode {
                    crate::queue::PlayMode::Sequential => "▶",
                    crate::queue::PlayMode::RepeatList => "🔁",
                    crate::queue::PlayMode::RepeatOne  => "🔂",
                    crate::queue::PlayMode::Shuffle    => "🔀",
                };
                self.set_status(label.to_string());
            }
            Command::FocusNext => {
                let (pv, dv) = self.column_visibility();
                self.focus_column = self.focus_column.next(pv, dv);
            }
            Command::FocusPrev => {
                let (pv, dv) = self.column_visibility();
                self.focus_column = self.focus_column.prev(pv, dv);
            }
            Command::MoveCursorUp => {
                if self.popup_stack.contains(&PopupLayer::Search) {
                    if self.search_focus_input {
                        // Already at top
                    } else {
                        if self.search_cursor == 0 {
                            self.search_focus_input = true;
                        } else {
                            self.search_cursor -= 1;
                        }
                    }
                } else {
                    self.move_cursor(-1);
                }
            }
            Command::MoveCursorDown => {
                if self.popup_stack.contains(&PopupLayer::Search) {
                    if self.search_focus_input {
                        if !self.search_results.is_empty() {
                            self.search_focus_input = false;
                            self.search_cursor = 0;
                        }
                    } else {
                        if self.search_cursor + 1 < self.search_results.len() {
                            self.search_cursor += 1;
                        }
                    }
                } else {
                    self.move_cursor(1);
                }
            }
            Command::MoveCursorLeft => {
                if self.popup_stack.contains(&PopupLayer::Search)
                    && self.search_focus_input
                    && self.search_query_cursor > 0
                {
                    self.search_query_cursor -= 1;
                }
            }
            Command::MoveCursorRight => {
                if self.popup_stack.contains(&PopupLayer::Search)
                    && self.search_focus_input
                    && self.search_query_cursor < self.search_query.chars().count()
                {
                    self.search_query_cursor += 1;
                }
            }
            Command::MoveCursorPageUp => {
                if self.popup_stack.contains(&PopupLayer::Search) {
                    self.search_cursor = self.search_cursor.saturating_sub(10);
                } else {
                    self.move_cursor(-10);
                }
            }
            Command::MoveCursorPageDown => {
                if self.popup_stack.contains(&PopupLayer::Search) {
                    self.search_cursor = (self.search_cursor + 10)
                        .min(self.search_results.len().saturating_sub(1));
                } else {
                    self.move_cursor(10);
                }
            }
            Command::MoveCursorTop => {
                if self.popup_stack.contains(&PopupLayer::Search) {
                    self.search_focus_input = true;
                    self.search_cursor = 0;
                } else {
                    match self.focus_column {
                        FocusColumn::Playlist => {
                            self.playlist_cursor = 0;
                            self.ui.playlist_list_state.select(Some(0));
                            self.ui.track_list_cursor = 0;
                        }
                        FocusColumn::TrackList => {
                            self.ui.track_list_cursor = 0;
                        }
                        _ => {}
                    }
                }
            }
            Command::MoveCursorBottom => {
                if self.popup_stack.contains(&PopupLayer::Search) {
                    if !self.search_results.is_empty() {
                        self.search_focus_input = false;
                        self.search_cursor = self.search_results.len() - 1;
                    }
                } else {
                    match self.focus_column {
                        FocusColumn::Playlist => {
                            let max = self.playlists.len();
                            self.playlist_cursor = max;
                            self.ui.playlist_list_state.select(Some(max));
                            self.ui.track_list_cursor = 0;
                        }
                        FocusColumn::TrackList => {
                            let len = self.active_track_list_len();
                            if len > 0 {
                                self.ui.track_list_cursor = len - 1;
                            }
                        }
                        _ => {}
                    }
                }
            }
            Command::PlaySelected => {
                if self.popup_stack.contains(&PopupLayer::Search) {
                    if self.search_focus_input {
                        // Enter in input field re-submits search
                        self.handle_command(Command::SearchSubmit);
                    } else if !self.search_results.is_empty()
                        && self.search_cursor < self.search_results.len()
                    {
                        let track = self.search_results[self.search_cursor].clone();
                        let title = track.title.clone();
                        if let Some(existing_idx) = self.queue.tracks().iter().position(|t| t.bvid == track.bvid) {
                            self.queue.jump_to(existing_idx);
                            self.ui.track_list_cursor = existing_idx;
                            let t = self.queue.current_track().cloned();
                            if let Some(t) = t {
                                self.play_track(t);
                            }
                        } else {
                            self.queue.push(track.clone());
                            let idx = self.queue.len() - 1;
                            self.queue.jump_to(idx);
                            self.play_track(track);
                        }
                        self.set_status(format!("Playing: {title}"));
                    }
                } else {
                    self.play_selected();
                }
            }
            Command::AddToQueue => {
                if self.popup_stack.contains(&PopupLayer::Search) {
                    if !self.search_results.is_empty() && self.search_cursor < self.search_results.len() {
                        let track = self.search_results[self.search_cursor].clone();
                        self.add_track_to_queue_dedup(track);
                    }
                } else if self.focus_column == FocusColumn::TrackList && self.playlist_cursor > 0 {
                    let pl_idx = self.playlist_cursor - 1;
                    if pl_idx < self.playlists.len() {
                        let track_idx = self.ui.track_list_cursor;
                        if track_idx < self.playlists[pl_idx].tracks.len() {
                            let track = self.playlists[pl_idx].tracks[track_idx].clone();
                            self.add_track_to_queue_dedup(track);
                        }
                    }
                }
            }
            Command::RemoveFromQueue => {
                if self.focus_column == FocusColumn::TrackList {
                    if self.playlist_cursor == 0 {
                        // Queue view: remove from queue
                        let idx = self.ui.track_list_cursor;
                        if idx < self.queue.len() {
                            self.queue.remove(idx);
                            if self.ui.track_list_cursor > 0
                                && self.ui.track_list_cursor >= self.queue.len()
                            {
                                self.ui.track_list_cursor = self.queue.len().saturating_sub(1);
                            }
                        }
                    } else {
                        // Playlist view: remove from playlist
                        self.handle_command(Command::RemoveFromPlaylist);
                    }
                }
            }
            Command::OpenSearch => {
                self.input_mode = InputMode::SearchInput;
                self.search_query.clear();
                self.search_query_cursor = 0;
                self.search_results.clear();
                self.search_cursor = 0;
                self.search_focus_input = true;
                self.show_popup(PopupLayer::Search);
            }
            Command::CloseSearch => {
                self.input_mode = InputMode::Normal;
                self.hide_popup(PopupLayer::Search);
            }
            Command::EnterSearchInput => {
                self.input_mode = InputMode::SearchInput;
                self.search_focus_input = true;
            }
            Command::EnterSearchAppend => {
                self.input_mode = InputMode::SearchInput;
                self.search_focus_input = true;
                if self.search_query_cursor < self.search_query.chars().count() {
                    self.search_query_cursor += 1;
                }
            }
            Command::SearchInput(c) => {
                let char_idx = self.search_query_cursor;
                if char_idx >= self.search_query.chars().count() {
                    self.search_query.push(c);
                } else {
                    let byte_idx = self.search_query.char_indices().nth(char_idx).map(|(i, _)| i).unwrap_or(self.search_query.len());
                    self.search_query.insert(byte_idx, c);
                }
                self.search_query_cursor += 1;
            }
            Command::SearchBackspace => {
                if self.search_query_cursor > 0 {
                    let char_idx = self.search_query_cursor - 1;
                    let byte_idx = self.search_query.char_indices().nth(char_idx).map(|(i, _)| i).unwrap_or(0);
                    self.search_query.remove(byte_idx);
                    self.search_query_cursor -= 1;
                }
            }
            Command::SearchSubmit => {
                let query = self.search_query.clone();
                if !query.is_empty() {
                    // Detect BV ID: bare "BV..." or embedded in a bilibili.com URL
                    let bvid = extract_bvid(&query);
                    if let Some(bvid) = bvid {
                        // Direct BV playback: fetch video info, then play immediately
                        self.searching = true;
                        let client = self.client.clone();
                        let (tx, rx) = mpsc::unbounded_channel();
                        let bvid_clone = bvid.clone();
                        tokio::spawn(async move {
                            let result = get_video_info(&client, &bvid_clone).await;
                            let _ = tx.send(result);
                        });
                        self.pending_bv_fetch = Some(rx);
                        self.set_status(format!("Loading {bvid}…"));
                    } else {
                        // Regular keyword search
                        self.searching = true;
                        let client = self.client.clone();
                        let (result_tx, result_rx) = mpsc::unbounded_channel();

                        tokio::spawn(async move {
                            let result = search_videos(&client, &query, 1).await;
                            let _ = result_tx.send(result);
                        });

                        self.search_result_rx = Some(result_rx);
                    }
                }
            }
            Command::OpenHelp => {
                self.show_popup(PopupLayer::Help);
            }
            Command::CloseHelp => {
                self.hide_popup(PopupLayer::Help);
            }
            Command::CreatePlaylist => {
                if self.focus_column == FocusColumn::Playlist {
                    self.playlist_name_input.clear();
                    self.show_popup(PopupLayer::PlaylistCreate);
                }
            }
            Command::CreatePlaylistChar(c) => {
                self.playlist_name_input.push(c);
            }
            Command::CreatePlaylistBackspace => {
                self.playlist_name_input.pop();
            }
            Command::CreatePlaylistConfirm => {
                let name = self.playlist_name_input.trim().to_string();
                if name.is_empty() {
                    self.set_status("Playlist name cannot be empty".to_string());
                } else {
                    self.playlists.push(Playlist::new(name));
                    self.playlist_cursor = self.playlists.len();
                    self.ui.playlist_list_state.select(Some(self.playlist_cursor));
                    self.playlist_name_input.clear();
                    self.hide_popup(PopupLayer::PlaylistCreate);
                    self.save_playlists_async();
                }
            }
            Command::CreatePlaylistCancel => {
                self.playlist_name_input.clear();
                self.hide_popup(PopupLayer::PlaylistCreate);
            }
            Command::DeletePlaylist => {
                if self.focus_column == FocusColumn::Playlist && self.playlist_cursor > 0 {
                    let idx = self.playlist_cursor - 1;
                    if idx < self.playlists.len() {
                        self.show_popup(PopupLayer::PlaylistDeleteConfirm);
                    }
                }
            }
            Command::DeletePlaylistConfirm => {
                let idx = self.playlist_cursor.saturating_sub(1);
                if self.playlist_cursor > 0 && idx < self.playlists.len() {
                    let name = self.playlists[idx].name.clone();
                    self.playlists.remove(idx);
                    if self.playlist_cursor > self.playlists.len() {
                        self.playlist_cursor = self.playlists.len();
                    }
                    self.ui.playlist_list_state.select(Some(self.playlist_cursor));
                    self.hide_popup(PopupLayer::PlaylistDeleteConfirm);
                    self.set_status(format!("Deleted playlist \"{}\"", name));
                    self.save_playlists_async();
                }
            }
            Command::DeletePlaylistCancel => {
                self.hide_popup(PopupLayer::PlaylistDeleteConfirm);
            }
            Command::AddToPlaylistOpen => {
                // Determine the track to add based on context
                let track = if self.popup_stack.contains(&PopupLayer::Search) {
                    if !self.search_results.is_empty() && self.search_cursor < self.search_results.len() {
                        Some(self.search_results[self.search_cursor].clone())
                    } else {
                        None
                    }
                } else if self.focus_column == FocusColumn::TrackList {
                    // Either from queue or playlist view
                    let tracks = self.active_track_list();
                    let idx = self.ui.track_list_cursor;
                    if idx < tracks.len() {
                        Some(tracks[idx].clone())
                    } else {
                        None
                    }
                } else {
                    None
                };
                if let Some(track) = track {
                    self.add_to_playlist_track = Some(track);
                    self.add_to_playlist_cursor = 0;
                    self.show_popup(PopupLayer::AddToPlaylist);
                } else {
                    self.set_status("No track selected".to_string());
                }
            }
            Command::AddToPlaylistMove(delta) => {
                let max = self.playlists.len();
                if max == 0 {
                    return;
                }
                let cur = self.add_to_playlist_cursor as isize;
                let new = (cur + delta).clamp(0, (max - 1) as isize) as usize;
                self.add_to_playlist_cursor = new;
            }
            Command::AddToPlaylistConfirm => {
                if let Some(track) = self.add_to_playlist_track.take() {
                    let idx = self.add_to_playlist_cursor;
                    if idx < self.playlists.len() {
                        let added = self.playlists[idx].add_track(track.clone());
                        let pl_name = self.playlists[idx].name.clone();
                        self.hide_popup(PopupLayer::AddToPlaylist);
                        if added {
                            self.set_status(format!("Added \"{}\" to \"{}\"", track.title, pl_name));
                            self.save_playlists_async();
                        } else {
                            self.set_status(format!("\"{}\" already in \"{}\"", track.title, pl_name));
                        }
                    }
                } else {
                    self.hide_popup(PopupLayer::AddToPlaylist);
                }
            }
            Command::AddToPlaylistCancel => {
                self.add_to_playlist_track = None;
                self.hide_popup(PopupLayer::AddToPlaylist);
            }
            Command::RemoveFromPlaylist => {
                if self.focus_column == FocusColumn::TrackList && self.playlist_cursor > 0 {
                    let pl_idx = self.playlist_cursor - 1;
                    if pl_idx < self.playlists.len() {
                        let track_idx = self.ui.track_list_cursor;
                        if track_idx < self.playlists[pl_idx].tracks.len() {
                            let removed = self.playlists[pl_idx].tracks.remove(track_idx);
                            if self.ui.track_list_cursor > 0
                                && self.ui.track_list_cursor >= self.playlists[pl_idx].tracks.len()
                            {
                                self.ui.track_list_cursor = self.playlists[pl_idx].tracks.len().saturating_sub(1);
                            }
                            self.set_status(format!("Removed \"{}\" from playlist", removed.title));
                            self.save_playlists_async();
                        }
                    }
                }
            }
            Command::FocusPlaylistColumn => {
                self.focus_column = FocusColumn::Playlist;
            }
            Command::FocusTrackListColumn => {
                self.focus_column = FocusColumn::TrackList;
                let len = self.active_track_list_len();
                if self.ui.track_list_cursor >= len && len > 0 {
                    self.ui.track_list_cursor = len - 1;
                } else if len == 0 {
                    self.ui.track_list_cursor = 0;
                }
            }
            Command::FocusDetailColumn => {
                self.focus_column = FocusColumn::Detail;
            }
            Command::Redraw => {
                // Rebuild cover protocol states.  For protocols that store pixel data
                // outside ratatui's cell buffer (Sixel, Kitty, iTerm2), also set
                // force_redraw so the render step calls terminal.clear() — which forces
                // ratatui to re-emit all escape sequences including the image data.
                // For Halfblocks, the Unicode chars live in ratatui's buffer and survive
                // tmux window switches, so no clear is needed (no flash for those users).
                let needs_clear = self.cover_manager
                    .as_ref()
                    .map(|mgr| mgr.needs_terminal_clear_on_focus())
                    .unwrap_or(true); // if no cover manager, Ctrl+L should still clear
                if let Some(mgr) = &mut self.cover_manager {
                    mgr.invalidate_all();
                }
                self.force_redraw = needs_clear;
            }
            Command::Noop => {}
        }
    }

    fn handle_tick(&mut self) {
        // Check for pending stream resolution
        if let Some((track, mut rx)) = self.pending_stream.take() {
            match rx.try_recv() {
                Ok(Ok(source)) => {
                    if let Err(e) = self.player.play(&source) {
                        self.set_status(format!("Playback error: {e}"));
                    } else {
                        // Update source info in the queue
                        if let Some(current) = self.queue.current_track_mut() {
                            current.source = Some(source);
                        }
                    }
                    self.is_playing = true;
                }
                Ok(Err(e)) => {
                    self.set_status(format!("Failed to get audio: {e}"));
                }
                Err(_) => {
                    self.pending_stream = Some((track, rx));
                }
            }
        }

        // Check for direct BV fetch result
        if let Some(mut rx) = self.pending_bv_fetch.take() {
            match rx.try_recv() {
                Ok(Ok(track)) => {
                    self.searching = false;
                    // Treat the result exactly like a 1-item search result list —
                    // the user chooses what to do next (Enter=play, a=add to queue,
                    // A=add to playlist, Esc=close).
                    self.search_results = vec![track];
                    self.search_cursor = 0;
                    self.search_focus_input = false;
                    self.input_mode = InputMode::SearchNormal;
                    self.status_message = None;
                }
                Ok(Err(e)) => {
                    self.searching = false;
                    self.set_status(format!("BV fetch failed: {e}"));
                    self.input_mode = InputMode::SearchInput;
                }
                Err(_) => {
                    // Still pending
                    self.pending_bv_fetch = Some(rx);
                }
            }
        }

        // Check for search results
        if let Some(mut rx) = self.search_result_rx.take() {
            match rx.try_recv() {
                Ok(result) => {
                    self.searching = false;
                    match result {
                        Ok(tracks) => {
                            self.search_results = tracks;
                            self.search_cursor = 0;
                            // Search successful, enter Normal mode for selection
                            self.input_mode = InputMode::SearchNormal;
                            // If results exist, move focus to results by default
                            if !self.search_results.is_empty() {
                                self.search_focus_input = false;
                            }
                        }
                        Err(e) => {
                            self.set_status(format!("Search failed: {e}"));
                            // Keep in Input mode on failure
                            self.input_mode = InputMode::SearchInput;
                        }
                    }
                }
                Err(_) => {
                    self.search_result_rx = Some(rx);
                }
            }
        }

        // Auto-dismiss volume slider after 3 seconds
        if self.popup_stack.contains(&PopupLayer::VolumeSlider)
            && let Some(ts) = self.volume_popup_time
            && ts.elapsed() > Duration::from_secs(3)
        {
            self.popup_stack.retain(|&l| l != PopupLayer::VolumeSlider);
            self.volume_popup_time = None;
        }
    }

    fn handle_player_event(&mut self, event: PlayerEvent) {
        match event {
            PlayerEvent::TrackEnded { reason } => {
                if reason == 0 {
                    if let Some(track) = self.queue.advance() {
                        let track = track.clone();
                        self.play_track(track);
                    } else {
                        self.is_playing = false;
                    }
                } else {
                    self.set_status(format!("Playback error (reason: {reason})"));
                    self.is_playing = false;
                }
            }
            PlayerEvent::SeekCompleted => {}
            PlayerEvent::Shutdown => {
                self.should_quit = true;
            }
        }
    }

    fn play_track(&mut self, track: Track) {
        let client = self.client.clone();
        let bvid = track.bvid.clone();
        let cid = track.cid;

        let (tx, rx) = mpsc::unbounded_channel();
        tokio::spawn(async move {
            let target_cid = if cid == 0 {
                match get_video_info(&client, &bvid).await {
                    Ok(info) => info.cid,
                    Err(e) => {
                        let _ = tx.send(Err(e));
                        return;
                    }
                }
            } else {
                cid
            };

            let result = get_audio_stream(&client, &bvid, target_cid).await;
            let _ = tx.send(result);
        });

        self.pending_stream = Some((track, rx));
    }

    fn play_selected(&mut self) {
        if self.focus_column == FocusColumn::TrackList {
            if self.playlist_cursor == 0 {
                // Queue view: play highlighted queue track
                let idx = self.ui.track_list_cursor;
                if idx < self.queue.len() {
                    self.queue.jump_to(idx);
                    if let Some(track) = self.queue.current_track().cloned() {
                        self.play_track(track);
                    }
                }
            } else {
                // Playlist view: replace queue with entire playlist, play from cursor
                let pl_idx = self.playlist_cursor - 1;
                if pl_idx < self.playlists.len() {
                    let start = self.ui.track_list_cursor;
                    let tracks = self.playlists[pl_idx].tracks.clone();
                    if !tracks.is_empty() && start < tracks.len() {
                        self.queue.clear();
                        for t in tracks {
                            self.queue.push(t);
                        }
                        self.queue.jump_to(start);
                        self.ui.track_list_cursor = start;
                        // Switch middle column view to Queue
                        self.playlist_cursor = 0;
                        self.ui.playlist_list_state.select(Some(0));
                        if let Some(track) = self.queue.current_track().cloned() {
                            self.play_track(track);
                        }
                    }
                }
            }
        }
    }

    fn move_cursor(&mut self, delta: isize) {
        match self.focus_column {
            FocusColumn::Playlist => {
                let max = self.playlists.len();
                let cur = self.playlist_cursor as isize;
                let new = (cur + delta).clamp(0, max as isize) as usize;
                self.playlist_cursor = new;
                self.ui.playlist_list_state.select(Some(new));
                self.ui.track_list_cursor = 0;
                // Suppress cover re-render while scrolling the playlist column
                self.cover_render_after = Some(
                    std::time::Instant::now() + std::time::Duration::from_millis(120),
                );
            }
            FocusColumn::TrackList => {
                let len = self.active_track_list_len();
                if len == 0 {
                    return;
                }
                let cur = self.ui.track_list_cursor as isize;
                let new = (cur + delta).clamp(0, (len - 1) as isize);
                self.ui.track_list_cursor = new as usize;
                // Suppress cover re-render while the user is scrolling quickly.
                // 120 ms after the last j/k the image will appear.
                self.cover_render_after = Some(
                    std::time::Instant::now() + std::time::Duration::from_millis(120),
                );
            }
            _ => {}
        }
    }

    // Popup management
    pub(crate) fn show_popup(&mut self, layer: PopupLayer) {
        if layer == PopupLayer::VolumeSlider {
            self.volume_popup_time = Some(std::time::Instant::now());
        }
        if !self.popup_stack.contains(&layer) {
            self.popup_stack.push(layer);
        } else if layer == PopupLayer::VolumeSlider {
            self.volume_popup_time = Some(std::time::Instant::now());
        }
    }

    fn hide_popup(&mut self, layer: PopupLayer) {
        self.popup_stack.retain(|&l| l != layer);
        if layer == PopupLayer::VolumeSlider {
            self.volume_popup_time = None;
        }
    }

    pub fn is_popup_active(&self, layer: PopupLayer) -> bool {
        self.popup_stack.contains(&layer)
    }

    fn set_status(&mut self, msg: String) {
        self.status_message = Some(msg);
    }

    /// Length of the currently visible track list (queue if cursor=0, else playlist tracks).
    pub fn active_track_list_len(&self) -> usize {
        if self.playlist_cursor == 0 {
            self.queue.len()
        } else {
            let pl_idx = self.playlist_cursor - 1;
            self.playlists.get(pl_idx).map(|p| p.tracks.len()).unwrap_or(0)
        }
    }

    /// Slice of tracks for the currently visible middle column.
    pub fn active_track_list(&self) -> &[Track] {
        if self.playlist_cursor == 0 {
            self.queue.tracks()
        } else {
            let pl_idx = self.playlist_cursor - 1;
            self.playlists.get(pl_idx).map(|p| p.tracks.as_slice()).unwrap_or(&[])
        }
    }

    fn save_playlists_async(&mut self) {
        let store = PlaylistStore { playlists: self.playlists.clone() };
        tokio::spawn(async move {
            if let Err(e) = store.save() {
                eprintln!("Failed to save playlists: {e}");
            }
        });
    }

    fn add_track_to_queue_dedup(&mut self, track: Track) {
        if let Some(existing_idx) = self.queue.tracks().iter().position(|t| t.bvid == track.bvid) {
            self.queue.jump_to(existing_idx);
            self.ui.track_list_cursor = existing_idx;
            let t = self.queue.current_track().cloned();
            if let Some(t) = t {
                self.play_track(t);
            }
            self.set_status(format!("Already in queue – jumped to: {}", track.title));
        } else {
            self.queue.push(track.clone());
            self.set_status(format!("Added to queue: {}", track.title));
        }
    }

    fn column_visibility(&self) -> (bool, bool) {
        if self.terminal_width >= 80 {
            (true, true)
        } else if self.terminal_width >= 50 {
            (true, false)
        } else {
            (false, false)
        }
    }

    /// Draw the entire UI. Splits borrows to avoid self conflict.
    pub(crate) fn draw(&mut self, f: &mut ratatui::Frame) {
        let area = f.area();
        self.terminal_width = area.width;
        self.terminal_height = area.height;

        let visibility = crate::ui::layout::ColumnVisibility::from_width(area.width);
        let (header, body, status) = crate::ui::layout::main_layout(area);
        let (playlist, track_list, detail) = crate::ui::layout::body_columns(body, visibility);

        crate::ui::draw_header(f, self, header);
        crate::ui::draw_status_bar(f, self, status);

        crate::ui::playlist_view::draw(f, self, playlist);

        let mut cover_manager = self.cover_manager.take();
        crate::ui::now_playing::draw(f, self, &mut cover_manager, detail);
        self.cover_manager = cover_manager;

        let mut ui = std::mem::take(&mut self.ui);
        crate::ui::track_list::draw(f, self, &mut ui, track_list);

        if self.popup_stack.contains(&PopupLayer::VolumeSlider) {
            crate::ui::volume_slider::draw(f, self, area);
        }
        if self.popup_stack.contains(&PopupLayer::Search) {
            crate::ui::search_view::draw(f, self, area);
        }
        if self.popup_stack.contains(&PopupLayer::Help) {
            crate::ui::help_view::draw(f, self, area);
        }
        if self.popup_stack.contains(&PopupLayer::PlaylistCreate) {
            crate::ui::input_popup::draw(f, self, area);
        }
        if self.popup_stack.contains(&PopupLayer::PlaylistDeleteConfirm) {
            crate::ui::confirm_popup::draw(f, self, area);
        }
        if self.popup_stack.contains(&PopupLayer::AddToPlaylist) {
            crate::ui::add_to_playlist_popup::draw(f, self, area);
        }

        self.ui = ui;
    }
}

// ── tmux focus-events helpers ─────────────────────────────────────────────────
//
// When running inside tmux, `focus-events` must be `on` for the terminal's
// \033[?1004h (EnableFocusChange) sequence to be forwarded to panes.  We
// enable it on entry and restore whatever the user had before on exit.

/// Enable `tmux focus-events` for the current session.
/// Returns `true` if it was already on (so the caller knows not to turn it off).
/// Returns `false` if it was off (we turned it on, caller should turn it off later).
/// Returns `false` also when not inside tmux — no-op.
fn enable_tmux_focus_events() -> bool {
    // If not inside tmux, do nothing.
    if std::env::var("TMUX").is_err() {
        return true; // "was already in desired state" → no action needed on exit
    }

    // Query current value
    let already_on = std::process::Command::new("tmux")
        .args(["show-option", "-gv", "focus-events"])
        .output()
        .map(|o| String::from_utf8_lossy(&o.stdout).trim() == "on")
        .unwrap_or(false);

    if !already_on {
        let _ = std::process::Command::new("tmux")
            .args(["set-option", "-g", "focus-events", "on"])
            .status();
    }

    already_on
}

/// Restore `tmux focus-events` to the state it was in before we ran.
/// `was_on` is the value returned by `enable_tmux_focus_events()`.
fn restore_tmux_focus_events(was_on: bool) {
    if std::env::var("TMUX").is_err() {
        return;
    }
    if !was_on {
        // We turned it on — turn it back off
        let _ = std::process::Command::new("tmux")
            .args(["set-option", "-g", "focus-events", "off"])
            .status();
    }
}

impl Default for App {
    fn default() -> Self {
        Self::new(Config::default(), None).unwrap()
    }
}

#[cfg(test)]
mod tests {
    use super::extract_bvid;

    #[test]
    fn test_extract_bvid_bare() {
        assert_eq!(extract_bvid("BV1xx411c7mD"), Some("BV1xx411c7mD".to_string()));
    }

    #[test]
    fn test_extract_bvid_url() {
        assert_eq!(
            extract_bvid("https://www.bilibili.com/video/BV1xx411c7mD"),
            Some("BV1xx411c7mD".to_string())
        );
    }

    #[test]
    fn test_extract_bvid_url_with_query() {
        assert_eq!(
            extract_bvid("https://www.bilibili.com/video/BV1xx411c7mD?spm_id_from=333.999"),
            Some("BV1xx411c7mD".to_string())
        );
    }

    #[test]
    fn test_extract_bvid_not_found() {
        assert_eq!(extract_bvid("周杰伦 晴天"), None);
        assert_eq!(extract_bvid(""), None);
        assert_eq!(extract_bvid("BV123"), None); // too short
    }

    #[test]
    fn test_extract_bvid_case_sensitive() {
        // BV IDs are case-sensitive — lowercase "bv" should not match
        assert_eq!(extract_bvid("bv1xx411c7mD"), None);
    }
}
