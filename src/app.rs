use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::time::Duration;

use crossterm::{
    event::{DisableFocusChange, EnableFocusChange, Event as CrosstermEvent, KeyCode, KeyEvent},
    execute,
    terminal::{EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode, enable_raw_mode},
};
use ratatui::{Terminal, backend::CrosstermBackend};
use tokio::sync::mpsc;

use crate::bilibili::api::BilibiliClient;
use crate::bilibili::search::search_videos;
use crate::bilibili::stream::get_audio_stream;
use crate::bilibili::video::{VideoPage, get_video_info, get_video_pages};
use crate::command::Command;
use crate::config::Config;
use crate::cover::CoverManager;
use tracing::debug;

use crate::bilibili::favorite::{
    FavoriteFolderSnapshot, fetch_favorites_snapshot, parse_favorite_folder_id,
};
use crate::event::{EndReason, PlayerEvent};
use crate::player::mpv::MpvBackend;
use crate::playlist::Playlist;
use crate::playlist::storage::PlaylistStore;
use crate::queue::Queue;
use crate::queue::plan::{self, ResolvedPlan};
use crate::queue::track::Track;
use crate::state::AppState;
use crate::ui::Ui;

/// Input mode for key handling.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InputMode {
    Normal,
    SearchInput,
    SearchNormal,
    SearchResults,
}

impl InputMode {
    pub fn search_focus_input(self) -> bool {
        matches!(self, Self::SearchInput | Self::SearchNormal)
    }

    /// Escape unwinds one search layer at a time.
    pub fn search_escape(self) -> Self {
        match self {
            Self::SearchInput | Self::SearchResults => Self::SearchNormal,
            Self::SearchNormal | Self::Normal => Self::Normal,
        }
    }

    pub fn search_submitted(self, query: &str) -> Self {
        if query.trim().is_empty() {
            self
        } else {
            Self::SearchResults
        }
    }

    /// An async response must preserve any editing/navigation done since submit.
    pub fn search_finished(self, has_results: bool) -> Self {
        if self == Self::SearchResults && !has_results {
            Self::SearchNormal
        } else {
            self
        }
    }
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

/// A row in the middle column.
///
/// The middle column is a tree: every track is a row, and a multi-part video
/// that is expanded also owns one row per part (分P).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TrackRow {
    /// A track (video) row.  Payload is the index into `active_track_list()`.
    Video(usize),
    /// A part row.  Payload is (track index, part index into `pages_cache[bvid]`).
    Page(usize, usize),
    /// Title row above a run of part-level tracks belonging to one video.
    ///
    /// Entering a multi-part video from a playlist fills the queue with its
    /// parts, so consecutive videos blur into one undifferentiated list of
    /// `P1 · …` rows.  Not a queue item of its own -- it exists purely so each
    /// run of parts is labelled with the video it came from.
    PartGroupHeader(usize),
}

impl TrackRow {
    /// Index of the track this row belongs to.
    pub fn track_index(self) -> usize {
        match self {
            TrackRow::Video(i) | TrackRow::Page(i, _) | TrackRow::PartGroupHeader(i) => i,
        }
    }
}

/// How many part-list requests may be in flight at once.
///
/// `/x/player/pagelist` is cheap but B 站 throttles aggressively (HTTP 412), so
/// prefetching a whole result list at once is not an option.
const MAX_PAGES_IN_FLIGHT: usize = 4;

/// How many leading tracks of the visible list to prefetch part lists for.
const PREFETCH_LOOKAHEAD: usize = 30;

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
    pub help_scroll: usize,
    /// The focus to restore when leaving the fullscreen detail view.
    pub detail_fullscreen_return_focus: Option<FocusColumn>,

    // Search state
    pub search_query: String,
    pub search_query_cursor: usize,
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
    pub search_result_rx:
        Option<mpsc::UnboundedReceiver<Result<Vec<Track>, crate::error::BilibiliError>>>,
    pub pending_stream: Option<(
        Track,
        mpsc::UnboundedReceiver<
            Result<crate::queue::track::TrackSource, crate::error::BilibiliError>,
        >,
    )>,
    /// Pending BV fetch: resolves video info for a directly-entered BV ID / URL.
    pub pending_bv_fetch:
        Option<mpsc::UnboundedReceiver<Result<Track, crate::error::BilibiliError>>>,

    // Multi-part (分P) state
    //
    // Neither search endpoint reports the part count, so the only way to learn
    // that a video is multi-part is to ask `/x/player/pagelist`.  We prefetch
    // those lists in the background for the visible list and cache them here.
    /// Resolved part lists, keyed by bvid.  A 1-element vec means "single part".
    pub pages_cache: HashMap<String, Vec<VideoPage>>,
    /// Sender/receiver shared by every in-flight part-list request.
    pages_tx: mpsc::UnboundedSender<(String, Result<Vec<VideoPage>, crate::error::BilibiliError>)>,
    pages_rx:
        mpsc::UnboundedReceiver<(String, Result<Vec<VideoPage>, crate::error::BilibiliError>)>,
    /// bvids whose part list is currently being fetched.
    pages_loading: HashSet<String>,
    pages_retry_after: HashMap<String, std::time::Instant>,
    /// bvids whose part list is expanded in the middle column.
    pub expanded: HashSet<String>,
    /// Set when the user explicitly toggled a row whose part list was not
    /// loaded yet — expand it as soon as the request lands.
    pending_expand: Option<String>,
    pending_enter: Option<(usize, usize, String)>,

    pending_queue_plan: Option<(
        QueueAction,
        mpsc::UnboundedReceiver<Result<ResolvedPlan, crate::error::BilibiliError>>,
        tokio::task::JoinHandle<()>,
    )>,
    pending_favorites: Option<(
        mpsc::UnboundedReceiver<Result<FavoriteFolderSnapshot, crate::error::BilibiliError>>,
        tokio::task::JoinHandle<()>,
    )>,
    pub importing_favorites: bool,

    // Cover image manager (None if terminal doesn't support graphics)
    pub cover_manager: Option<CoverManager>,

    /// Set to `true` to force `terminal.clear()` before the next draw.
    /// Used by Ctrl+L (manual redraw) and automatic tmux focus-regain handling.
    pub force_redraw: bool,

    /// Enabled by run(); tests can use an isolated directory.
    persistence_dir: Option<PathBuf>,

    /// When `Some`, cover rendering is suppressed until this instant passes.
    /// Set on every cursor movement; expires 100 ms after the last move.
    /// This prevents the terminal from transmitting raw pixel data (Kitty/Sixel)
    /// on every j/k keypress, which causes noticeable input lag.
    pub cover_render_after: Option<std::time::Instant>,
}

enum QueueAction {
    Replace {
        playlist: usize,
        original: Vec<Track>,
    },
    Append {
        play: bool,
    },
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
    pub fn new(
        config: Config,
        picker: Option<ratatui_image::picker::Picker>,
    ) -> anyhow::Result<Self> {
        Self::new_with_playlists(
            config,
            picker,
            PlaylistStore::load()
                .map(|s| s.playlists)
                .unwrap_or_default(),
        )
    }

    pub fn new_with_playlists(
        config: Config,
        picker: Option<ratatui_image::picker::Picker>,
        playlists: Vec<Playlist>,
    ) -> anyhow::Result<Self> {
        let logged_in = !config.bilibili.sessdata.is_empty();
        let client = BilibiliClient::new(Some(config.bilibili.sessdata.clone()));
        let mut player = MpvBackend::new()?;
        let volume = config.player.volume;
        player.set_volume(volume);
        let (pages_tx, pages_rx) = mpsc::unbounded_channel();

        let mut app = Self {
            config,
            client,
            player,
            queue: Queue::new(),
            playlists,
            playlist_cursor: 0,
            playlist_name_input: String::new(),
            add_to_playlist_cursor: 0,
            add_to_playlist_track: None,
            ui: Ui::new(),
            input_mode: InputMode::Normal,
            focus_column: FocusColumn::TrackList,
            help_scroll: 0,
            detail_fullscreen_return_focus: None,
            search_query: String::new(),
            search_query_cursor: 0,
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
            pages_cache: HashMap::new(),
            pages_tx,
            pages_rx,
            pages_loading: HashSet::new(),
            pages_retry_after: HashMap::new(),
            expanded: HashSet::new(),
            pending_expand: None,
            pending_enter: None,
            pending_queue_plan: None,
            pending_favorites: None,
            importing_favorites: false,
            cover_manager: picker.map(CoverManager::new),
            force_redraw: false,
            persistence_dir: None,
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
        self.persistence_dir = Some(Config::config_dir()?);

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
                    // Forward keys, focus gains, and resizes so layout updates immediately.
                    if matches!(
                        event,
                        CrosstermEvent::Key(_)
                            | CrosstermEvent::FocusGained
                            | CrosstermEvent::Resize(_, _)
                    ) && key_tx.send(event).is_err()
                    {
                        break;
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
            Ok(state) => {
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
            Err(e) => {
                // Non-fatal: just log and continue with an empty queue
                eprintln!("Warning: failed to load state: {e}");
            }
        }

        let result = self
            .run_loop(&mut terminal, &mut key_rx, &mut tick_interval)
            .await;

        // Save before terminal cleanup: cleanup failures must not skip persistence.
        self.save_queue();
        self.save_playlists();

        // Restore terminal
        disable_raw_mode()?;
        execute!(
            terminal.backend_mut(),
            LeaveAlternateScreen,
            DisableFocusChange
        )?;
        restore_tmux_focus_events(tmux_focus_was_on);
        let _ = self.player.shutdown();

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
                        Some(CrosstermEvent::Resize(width, height)) => {
                            self.terminal_width = width;
                            self.terminal_height = height;
                            self.normalize_focus();
                            self.force_redraw = true;
                        }
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
                if let Some(manager) = &mut self.cover_manager {
                    manager.invalidate_all();
                }
                let _ = terminal.clear();
            }
            terminal.draw(|f| self.draw(f))?;
        }

        Ok(())
    }

    fn handle_key(&mut self, key: KeyEvent) {
        // Modal popups and the transient volume overlay get the first back key.
        if self.detail_fullscreen_return_focus.is_some()
            && self
                .popup_stack
                .iter()
                .all(|layer| *layer == PopupLayer::VolumeSlider)
            && self.input_mode == InputMode::Normal
        {
            if (key.code == KeyCode::Esc
                || (key.code == KeyCode::Char('q') && key.modifiers.is_empty()))
                && self.popup_stack.is_empty()
            {
                self.handle_command(Command::CloseDetailFullscreen);
                return;
            }
            if matches!(
                key.code,
                KeyCode::Tab | KeyCode::BackTab | KeyCode::Enter | KeyCode::Char('h' | 'l')
            ) {
                return;
            }
        }
        // Dismiss the top volume overlay before leaving fullscreen detail.
        if self.popup_stack.last() == Some(&PopupLayer::VolumeSlider)
            && (key.code == KeyCode::Esc
                || (self.detail_fullscreen_return_focus.is_some()
                    && key.code == KeyCode::Char('q')
                    && key.modifiers.is_empty()))
        {
            self.hide_popup(PopupLayer::VolumeSlider);
            return;
        }

        if key.code == KeyCode::Esc
            && (self.pending_queue_plan.is_some()
                || self.pending_favorites.is_some()
                || self.pending_enter.is_some())
        {
            self.cancel_pending_work();
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
        let previous_queue = self.persistence_dir.as_ref().map(|_| self.queue.snapshot());
        if matches!(
            cmd,
            Command::Quit
                | Command::PlaySelected
                | Command::NextTrack
                | Command::PrevTrack
                | Command::RemoveFromQueue
                | Command::RemoveFromPlaylist
                | Command::DeletePlaylistConfirm
        ) {
            if let Some((_, _, task)) = self.pending_queue_plan.take() {
                task.abort();
            }
        }
        if matches!(cmd, Command::SearchEscape | Command::CloseSearch)
            && self.pending_queue_plan.is_some()
        {
            self.cancel_pending_work();
        }
        let old_search = (self.input_mode, self.search_cursor);
        self.apply_command(cmd);
        if old_search != (self.input_mode, self.search_cursor) {
            self.ui.search_marquee.reset();
        }
        if previous_queue.is_some_and(|previous| previous != self.queue.snapshot()) {
            self.save_queue();
        }
    }

    fn apply_command(&mut self, cmd: Command) {
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
                self.set_status(self.queue.play_mode.icon().to_string());
            }
            Command::ToggleDetailFullscreen => {
                if self.detail_fullscreen_return_focus.is_some() {
                    self.close_detail_fullscreen();
                } else {
                    self.detail_fullscreen_return_focus = Some(self.focus_column);
                    self.focus_column = FocusColumn::Detail;
                    self.force_redraw = true;
                }
            }
            Command::CloseDetailFullscreen => self.close_detail_fullscreen(),
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
                    if self.input_mode.search_focus_input() {
                        // Already at top
                    } else {
                        if self.search_cursor == 0 {
                            self.input_mode = InputMode::SearchNormal;
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
                    if self.input_mode.search_focus_input() {
                        if !self.search_results.is_empty() {
                            self.input_mode = InputMode::SearchResults;
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
                    && self.input_mode.search_focus_input()
                    && self.search_query_cursor > 0
                {
                    self.search_query_cursor -= 1;
                }
            }
            Command::MoveCursorRight => {
                if self.popup_stack.contains(&PopupLayer::Search)
                    && self.input_mode.search_focus_input()
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
                    self.search_cursor =
                        (self.search_cursor + 10).min(self.search_results.len().saturating_sub(1));
                } else {
                    self.move_cursor(10);
                }
            }
            Command::MoveCursorTop => {
                if self.popup_stack.contains(&PopupLayer::Search) {
                    if self.input_mode.search_focus_input() {
                        self.search_query_cursor = 0;
                    } else {
                        self.search_cursor = 0;
                    }
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
                    if self.input_mode.search_focus_input() {
                        self.search_query_cursor = self.search_query.chars().count();
                    } else if !self.search_results.is_empty() {
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
                            let len = self.visible_row_len();
                            if len > 0 {
                                self.ui.track_list_cursor = len - 1;
                            }
                        }
                        _ => {}
                    }
                }
            }
            Command::ToggleExpand => {
                if !self.popup_stack.contains(&PopupLayer::Search) {
                    self.toggle_expand_current();
                }
            }
            Command::PlaySelected => {
                if self.popup_stack.contains(&PopupLayer::Search) {
                    if self.input_mode.search_focus_input() {
                        // Enter in input field re-submits search
                        self.handle_command(Command::SearchSubmit);
                    } else if !self.search_results.is_empty()
                        && self.search_cursor < self.search_results.len()
                    {
                        let track = self.search_results[self.search_cursor].clone();
                        self.start_queue_plan(
                            vec![track],
                            0,
                            None,
                            QueueAction::Append { play: true },
                        );
                    }
                } else {
                    self.play_selected();
                }
            }
            Command::AddToQueue => {
                if self.popup_stack.contains(&PopupLayer::Search) {
                    if !self.search_results.is_empty()
                        && self.search_cursor < self.search_results.len()
                    {
                        let track = self.search_results[self.search_cursor].clone();
                        self.start_queue_plan(
                            vec![track],
                            0,
                            None,
                            QueueAction::Append { play: false },
                        );
                    }
                } else if self.focus_column == FocusColumn::TrackList && self.playlist_cursor > 0 {
                    // Row-aware: a part row queues just that part, a video row
                    // queues the whole video (all parts).
                    if let Some(row) = self.current_row() {
                        self.add_row_to_queue(row);
                    }
                }
            }
            Command::RemoveFromQueue => {
                if self.focus_column == FocusColumn::TrackList {
                    if self.playlist_cursor == 0 {
                        // Queue view: remove from queue. Only video rows are
                        // queue items — part rows are derived from their parent.
                        if let Some(TrackRow::Video(idx)) = self.current_row()
                            && idx < self.queue.len()
                        {
                            self.queue.remove(idx);
                            let rows = self.visible_row_len();
                            if self.ui.track_list_cursor >= rows {
                                self.ui.track_list_cursor = rows.saturating_sub(1);
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
                self.show_popup(PopupLayer::Search);
            }
            Command::SearchEscape => {
                self.input_mode = self.input_mode.search_escape();
                if self.input_mode == InputMode::Normal {
                    self.handle_command(Command::CloseSearch);
                }
            }
            Command::CloseSearch => {
                self.ui.search_marquee.reset();
                self.search_result_rx = None;
                self.pending_bv_fetch = None;
                self.searching = false;
                self.input_mode = InputMode::Normal;
                self.hide_popup(PopupLayer::Search);
            }
            Command::EnterSearchInput => {
                self.input_mode = InputMode::SearchInput;
            }
            Command::EnterSearchAppend => {
                self.input_mode = InputMode::SearchInput;
                if self.search_query_cursor < self.search_query.chars().count() {
                    self.search_query_cursor += 1;
                }
            }
            Command::SearchInput(c) => {
                let char_idx = self.search_query_cursor;
                if char_idx >= self.search_query.chars().count() {
                    self.search_query.push(c);
                } else {
                    let byte_idx = self
                        .search_query
                        .char_indices()
                        .nth(char_idx)
                        .map(|(i, _)| i)
                        .unwrap_or(self.search_query.len());
                    self.search_query.insert(byte_idx, c);
                }
                self.search_query_cursor += 1;
            }
            Command::SearchBackspace => {
                if self.search_query_cursor > 0 {
                    let char_idx = self.search_query_cursor - 1;
                    let byte_idx = self
                        .search_query
                        .char_indices()
                        .nth(char_idx)
                        .map(|(i, _)| i)
                        .unwrap_or(0);
                    self.search_query.remove(byte_idx);
                    self.search_query_cursor -= 1;
                }
            }
            Command::SearchSubmit => {
                self.ui.search_marquee.reset();
                let query = self.search_query.clone();
                if !query.trim().is_empty() {
                    // A newer submission owns the results, including switching
                    // between keyword search and direct BV lookup.
                    self.search_result_rx = None;
                    self.pending_bv_fetch = None;
                    self.search_results.clear();
                    self.search_cursor = 0;
                    self.input_mode = self.input_mode.search_submitted(&query);
                    // Detect BV ID: bare "BV..." or embedded in a bilibili.com URL
                    let bvid = extract_bvid(&query);
                    if let Some(bvid) = bvid {
                        // Direct BV lookup: fetch a single selectable result
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
            Command::HelpScroll(delta) => {
                let screen =
                    ratatui::layout::Rect::new(0, 0, self.terminal_width, self.terminal_height);
                let limit = crate::ui::help_view::scroll_limit(screen);
                let current = self.help_scroll.min(limit);
                self.help_scroll = current.saturating_add_signed(delta as isize).min(limit);
            }
            Command::HelpPage(direction) => {
                let screen =
                    ratatui::layout::Rect::new(0, 0, self.terminal_width, self.terminal_height);
                let limit = crate::ui::help_view::scroll_limit(screen);
                let page = crate::ui::help_view::page_height(screen);
                self.help_scroll = self
                    .help_scroll
                    .min(limit)
                    .saturating_add_signed(direction as isize * page as isize)
                    .min(limit);
            }
            Command::HelpTop => self.help_scroll = 0,
            Command::HelpBottom => {
                let screen =
                    ratatui::layout::Rect::new(0, 0, self.terminal_width, self.terminal_height);
                self.help_scroll = crate::ui::help_view::scroll_limit(screen);
            }
            Command::OpenHelp => {
                self.help_scroll = 0;
                self.show_popup(PopupLayer::Help);
            }
            Command::CloseHelp => {
                self.hide_popup(PopupLayer::Help);
            }
            Command::ImportFavorites => {
                if self.focus_column == FocusColumn::Playlist {
                    self.cancel_pending_work();
                    self.importing_favorites = true;
                    self.playlist_name_input.clear();
                    self.show_popup(PopupLayer::PlaylistCreate);
                }
            }
            Command::CancelPending => self.cancel_pending_work(),
            Command::CreatePlaylist => {
                self.importing_favorites = false;
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
                if self.importing_favorites {
                    self.start_favorites_import();
                    return;
                }
                let name = self.playlist_name_input.trim().to_string();
                if name.is_empty() {
                    self.set_status("Playlist name cannot be empty".to_string());
                } else {
                    self.playlists.push(Playlist::new(name));
                    self.playlist_cursor = self.playlists.len();
                    self.ui
                        .playlist_list_state
                        .select(Some(self.playlist_cursor));
                    self.playlist_name_input.clear();
                    self.hide_popup(PopupLayer::PlaylistCreate);
                    self.save_playlists();
                }
            }
            Command::CreatePlaylistCancel => {
                self.cancel_pending_work();
                self.playlist_name_input.clear();
                self.importing_favorites = false;
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
                    self.ui
                        .playlist_list_state
                        .select(Some(self.playlist_cursor));
                    self.hide_popup(PopupLayer::PlaylistDeleteConfirm);
                    self.set_status(format!("Deleted playlist \"{}\"", name));
                    self.save_playlists();
                }
            }
            Command::DeletePlaylistCancel => {
                self.hide_popup(PopupLayer::PlaylistDeleteConfirm);
            }
            Command::AddToPlaylistOpen => {
                // Determine the track to add based on context
                let track = if self.popup_stack.contains(&PopupLayer::Search) {
                    if !self.search_results.is_empty()
                        && self.search_cursor < self.search_results.len()
                    {
                        Some(self.search_results[self.search_cursor].clone())
                    } else {
                        None
                    }
                } else if self.focus_column == FocusColumn::TrackList {
                    // Either from queue or playlist view; part rows stage the
                    // individual part, video rows stage the video.
                    self.current_row().and_then(|row| self.row_track(row))
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
                            self.set_status(format!(
                                "Added \"{}\" to \"{}\"",
                                track.title, pl_name
                            ));
                            self.save_playlists();
                        } else {
                            self.set_status(format!(
                                "\"{}\" already in \"{}\"",
                                track.title, pl_name
                            ));
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
                        // Only video rows are playlist entries — a part row is
                        // derived from its parent video and cannot be removed.
                        if let Some(TrackRow::Video(track_idx)) = self.current_row()
                            && track_idx < self.playlists[pl_idx].tracks.len()
                        {
                            let removed = self.playlists[pl_idx].tracks.remove(track_idx);
                            let rows = self.visible_row_len();
                            if self.ui.track_list_cursor > 0 && self.ui.track_list_cursor >= rows {
                                self.ui.track_list_cursor = rows.saturating_sub(1);
                            }
                            self.set_status(format!("Removed \"{}\" from playlist", removed.title));
                            self.save_playlists();
                        }
                    }
                }
            }
            Command::FocusPlaylistColumn => {
                if self.column_visibility().0 {
                    self.focus_column = FocusColumn::Playlist;
                }
            }
            Command::FocusTrackListColumn => {
                self.focus_column = FocusColumn::TrackList;
                let len = self.visible_row_len();
                if self.ui.track_list_cursor >= len && len > 0 {
                    self.ui.track_list_cursor = len - 1;
                } else if len == 0 {
                    self.ui.track_list_cursor = 0;
                }
            }
            Command::FocusDetailColumn => {
                if self.column_visibility().1 {
                    self.focus_column = FocusColumn::Detail;
                }
            }
            Command::Redraw => {
                // Rebuild cover protocol states.  For protocols that store pixel data
                // outside ratatui's cell buffer (Sixel, Kitty, iTerm2), also set
                // force_redraw so the render step calls terminal.clear() — which forces
                // ratatui to re-emit all escape sequences including the image data.
                // For Halfblocks, the Unicode chars live in ratatui's buffer and survive
                // tmux window switches, so no clear is needed (no flash for those users).
                let needs_clear = self
                    .cover_manager
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
        self.poll_pending_work();
        // Check for pending stream resolution
        if let Some((track, mut rx)) = self.pending_stream.take() {
            match rx.try_recv() {
                Ok(Ok(source)) => {
                    // Queue edits can invalidate a request while it resolves.
                    if !self
                        .queue
                        .current_track()
                        .is_some_and(|current| current.same_item(&track))
                    {
                        return;
                    }
                    let title = track.display_title().to_string();
                    if let Err(e) = self.player.play(&source) {
                        self.is_playing = false;
                        self.set_status(format!("Playback error: {e}"));
                    } else {
                        // Update source info in the queue
                        if let Some(current) = self.queue.current_track_mut() {
                            current.source = Some(source);
                        }
                        // Starting playback used to be silent, so a track that
                        // never played was indistinguishable from one that did.
                        self.set_status(format!("Playing: {title}"));
                        self.is_playing = true;
                    }
                }
                Ok(Err(e)) => {
                    self.set_status(format!("Failed to get audio: {e}"));
                }
                Err(mpsc::error::TryRecvError::Empty) => {
                    self.pending_stream = Some((track, rx));
                }
                Err(mpsc::error::TryRecvError::Disconnected) => {
                    // The resolving task ended without ever sending.  Putting
                    // the entry back would retry it forever, so the track would
                    // silently never play.
                    let title = track.display_title().to_string();
                    self.set_status(format!("Failed to get audio: no response for {title}"));
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
                    // A=add to playlist, Esc=return to the query).
                    self.ui.search_marquee.reset();
                    self.search_results = vec![track];
                    self.search_cursor = 0;
                    self.input_mode = self.input_mode.search_finished(true);
                    self.status_message = None;
                }
                Ok(Err(e)) => {
                    self.searching = false;
                    self.set_status(format!("BV fetch failed: {e}"));
                    self.input_mode = self.input_mode.search_finished(false);
                }
                Err(mpsc::error::TryRecvError::Empty) => {
                    self.pending_bv_fetch = Some(rx);
                }
                Err(mpsc::error::TryRecvError::Disconnected) => {
                    self.searching = false;
                    self.input_mode = self.input_mode.search_finished(false);
                    self.set_status("BV fetch failed: no response".to_string());
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
                            self.ui.search_marquee.reset();
                            self.search_results = tracks;
                            self.search_cursor = 0;
                            self.input_mode = self
                                .input_mode
                                .search_finished(!self.search_results.is_empty());
                        }
                        Err(e) => {
                            self.set_status(format!("Search failed: {e}"));
                            self.input_mode = self.input_mode.search_finished(false);
                        }
                    }
                }
                Err(mpsc::error::TryRecvError::Empty) => {
                    self.search_result_rx = Some(rx);
                }
                Err(mpsc::error::TryRecvError::Disconnected) => {
                    self.searching = false;
                    self.input_mode = self.input_mode.search_finished(false);
                    self.set_status("Search failed: no response".to_string());
                }
            }
        }

        // Drain completed part-list fetches.
        loop {
            let received = self.pages_rx.try_recv();
            // `explicit` = the user asked for this one (via 'e'), so failures and
            // "only one part" are worth reporting; silent prefetch errors are not.
            let (bvid, result, explicit) = match received {
                Ok((bvid, result)) => {
                    let explicit = self.pending_expand.as_deref() == Some(bvid.as_str());
                    if explicit {
                        self.pending_expand = None;
                    }
                    (bvid, result, explicit)
                }
                Err(_) => break,
            };

            self.pages_loading.remove(&bvid);
            match result {
                Ok(pages) => {
                    let count = pages.len();
                    let selected_row = self.current_row();
                    let enter =
                        self.pending_enter
                            .as_ref()
                            .is_some_and(|(playlist, cursor, bv)| {
                                *playlist == self.playlist_cursor
                                    && *cursor == self.ui.track_list_cursor
                                    && bv == &bvid
                            });
                    self.pages_cache.insert(bvid.clone(), pages);
                    self.restore_selected_row(selected_row);
                    if self
                        .pending_enter
                        .as_ref()
                        .is_some_and(|(_, _, bv)| bv == &bvid)
                    {
                        self.pending_enter = None;
                    }
                    if enter && count == 1 {
                        self.play_selected();
                        continue;
                    }
                    if explicit {
                        if count > 1 {
                            self.expanded.insert(bvid);
                        } else {
                            self.set_status("This video has only one part".to_string());
                        }
                    }
                }
                Err(e) => {
                    self.pages_retry_after
                        .insert(bvid, std::time::Instant::now() + Duration::from_secs(30));
                    if explicit {
                        self.set_status(format!("Unable to load parts: {e}"));
                    }
                }
            }
        }

        // Background-fetch part lists for the visible list so the ▸ markers show up.
        self.prewarm_pages();

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
        let previous_queue = self.persistence_dir.as_ref().map(|_| self.queue.snapshot());
        match event {
            PlayerEvent::TrackEnded { generation, .. }
            | PlayerEvent::CommandFailed { generation, .. }
                if generation != self.player.generation() => {}
            PlayerEvent::TrackEnded { reason, .. } => match reason {
                EndReason::Eof => {
                    // A manually selected replacement is already resolving.
                    // This EOF belongs to the outgoing track, not the new index.
                    if self.pending_stream.is_some() {
                        return;
                    }
                    if let Some(track) = self.queue.advance() {
                        let track = track.clone();
                        self.play_track(track);
                    } else {
                        self.is_playing = false;
                    }
                }
                // Replacing the file is how a new track starts, and mpv reports
                // the outgoing one as STOP.  Only ERROR is a real failure -- the
                // rest must leave `is_playing` alone, or the UI shows "stopped"
                // over audio that is in fact playing.
                EndReason::Stop | EndReason::Quit | EndReason::Redirect => {}
                EndReason::Unknown(other) => {
                    debug!("ignoring unknown mpv end-file reason {other}");
                }
                EndReason::Error => {
                    self.set_status("Playback error: mpv could not play this stream".to_string());
                    self.is_playing = false;
                }
            },
            PlayerEvent::CommandFailed {
                operation, message, ..
            } => {
                self.set_status(format!("Playback error ({operation}): {message}"));
                self.is_playing = false;
            }
            PlayerEvent::SeekCompleted => {}
            PlayerEvent::Shutdown => {
                self.should_quit = true;
            }
        }
        if previous_queue.is_some_and(|previous| previous != self.queue.snapshot()) {
            self.save_queue();
        }
    }

    fn cancel_pending_work(&mut self) {
        self.pending_enter = None;
        self.pending_expand = None;
        if let Some((_, _, task)) = self.pending_queue_plan.take() {
            task.abort();
        }
        if let Some((_, task)) = self.pending_favorites.take() {
            task.abort();
        }
        self.set_status("Loading cancelled".into());
    }

    fn start_queue_plan(
        &mut self,
        tracks: Vec<Track>,
        start: usize,
        page: Option<u32>,
        action: QueueAction,
    ) {
        if let Some((_, _, task)) = self.pending_queue_plan.take() {
            task.abort();
        }
        if tracks.is_empty() {
            return;
        }
        // A hot cache uses precisely the same pure planner, without waiting for a tick.
        if let Ok((resolved, selected)) = plan::materialize(&tracks, start, page, &self.pages_cache)
        {
            self.commit_queue_plan(
                action,
                ResolvedPlan {
                    tracks: resolved,
                    start: selected,
                    pages: HashMap::new(),
                },
            );
            return;
        }
        let client = self.client.clone();
        let pages = self.pages_cache.clone();
        let (tx, rx) = mpsc::unbounded_channel();
        let task = tokio::spawn(async move {
            let _ = tx.send(plan::resolve(client, tracks, start, page, pages).await);
        });
        self.pending_queue_plan = Some((action, rx, task));
        self.set_status("Loading all parts… Esc to cancel".into());
    }

    fn commit_queue_plan(&mut self, action: QueueAction, resolved: ResolvedPlan) {
        let selected_row = self.current_row();
        self.pages_cache.extend(resolved.pages);
        self.restore_selected_row(selected_row);
        match action {
            QueueAction::Replace { playlist, original } => {
                if !self
                    .playlists
                    .get(playlist)
                    .is_some_and(|p| p.tracks == original)
                {
                    return;
                }
                self.queue.clear();
                for track in resolved.tracks {
                    self.queue.push(track);
                }
                self.queue.jump_to(resolved.start);
                self.playlist_cursor = 0;
                self.ui.playlist_list_state.select(Some(0));
                self.ui.track_list_cursor = self.row_of_queue_index(resolved.start).unwrap_or(0);
                if let Some(track) = self.queue.current_track().cloned() {
                    self.play_track(track);
                }
            }
            QueueAction::Append { play } => {
                let selected = resolved.tracks.get(resolved.start).cloned();
                for track in resolved.tracks {
                    if !self.queue.tracks().iter().any(|t| t.same_item(&track)) {
                        self.queue.push(track);
                    }
                }
                if play {
                    if let Some(track) = selected {
                        if let Some(i) =
                            self.queue.tracks().iter().position(|t| t.same_item(&track))
                        {
                            self.queue.jump_to(i);
                            self.ui.track_list_cursor = self.row_of_queue_index(i).unwrap_or(i);
                            self.play_track(track);
                        }
                    }
                } else {
                    self.set_status("Added to queue".into());
                }
            }
        }
        self.save_queue();
    }

    fn start_favorites_import(&mut self) {
        let id = match parse_favorite_folder_id(&self.playlist_name_input) {
            Ok(id) => id,
            Err(e) => {
                self.set_status(format!("Invalid favorite folder: {e}"));
                return;
            }
        };
        if let Some((_, task)) = self.pending_favorites.take() {
            task.abort();
        }
        let client = self.client.clone();
        let (tx, rx) = mpsc::unbounded_channel();
        let task = tokio::spawn(async move {
            let _ = tx.send(fetch_favorites_snapshot(&client, id).await);
        });
        self.pending_favorites = Some((rx, task));
        self.playlist_name_input.clear();
        self.hide_popup(PopupLayer::PlaylistCreate);
        self.importing_favorites = false;
        self.set_status("Importing favorites… Esc to cancel".into());
    }

    fn poll_pending_work(&mut self) {
        if let Some((action, mut rx, task)) = self.pending_queue_plan.take() {
            match rx.try_recv() {
                Ok(Ok(plan)) => self.commit_queue_plan(action, plan),
                Ok(Err(e)) => {
                    self.set_status(format!("Unable to load parts: {e}; queue preserved"))
                }
                Err(mpsc::error::TryRecvError::Empty) => {
                    self.pending_queue_plan = Some((action, rx, task))
                }
                Err(_) => self.set_status("Part loading stopped; queue preserved".into()),
            }
        }
        if let Some((mut rx, task)) = self.pending_favorites.take() {
            match rx.try_recv() {
                Ok(Ok(snapshot)) => {
                    let base = if snapshot.folder.title.trim().is_empty() {
                        format!("Favorites {}", snapshot.folder.id)
                    } else {
                        snapshot.folder.title
                    };
                    let mut name = base.clone();
                    let mut suffix = 2;
                    while self.playlists.iter().any(|p| p.name == name) {
                        name = format!("{base} ({suffix})");
                        suffix += 1;
                    }
                    let mut playlist = Playlist::new(name);
                    for video in snapshot.videos {
                        playlist.add_track(Track {
                            bvid: video.bvid,
                            cid: 0,
                            title: video.title,
                            author: video.author,
                            duration: Duration::from_secs(video.duration_secs),
                            cover_url: video.cover_url,
                            part: None,
                            page: None,
                            source: None,
                        });
                    }
                    let count = playlist.tracks.len();
                    self.playlists.push(playlist);
                    self.playlist_cursor = self.playlists.len();
                    self.ui
                        .playlist_list_state
                        .select(Some(self.playlist_cursor));
                    self.ui.track_list_cursor = 0;
                    self.save_playlists();
                    self.set_status(format!(
                        "Imported {count} videos; skipped {}",
                        snapshot.skipped.len()
                    ));
                }
                Ok(Err(e)) => self.set_status(format!("Favorite import failed: {e}")),
                Err(mpsc::error::TryRecvError::Empty) => self.pending_favorites = Some((rx, task)),
                Err(_) => self.set_status("Favorite import stopped".into()),
            }
        }
    }

    fn play_track(&mut self, track: Track) {
        let client = self.client.clone();
        let bvid = track.bvid.clone();
        let cid = track.cid;
        let page = track.page;

        let (tx, rx) = mpsc::unbounded_channel();
        tokio::spawn(async move {
            let target_cid = if cid == 0 {
                match get_video_pages(&client, &bvid).await {
                    Ok(pages) => match pages.iter().find(|p| p.page == page.unwrap_or(1)) {
                        Some(p) => p.cid,
                        None => {
                            let _ = tx.send(Err(crate::error::BilibiliError::Parse(
                                "Requested part is unavailable".into(),
                            )));
                            return;
                        }
                    },
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

    /// Enter in the middle column.
    ///
    /// A row that can fold out a part list does so first -- in both views --
    /// so a specific P can be picked.  Anything else starts playback: from the
    /// queue entry, or from that row in a playlist.
    fn play_selected(&mut self) {
        if self.focus_column != FocusColumn::TrackList {
            return;
        }
        let Some(row) = self.current_row() else {
            return;
        };

        if matches!(row, TrackRow::Video(_))
            && self
                .active_track_list()
                .get(row.track_index())
                .is_some_and(|t| t.page.is_none() && !self.pages_cache.contains_key(&t.bvid))
        {
            let bvid = self.active_track_list()[row.track_index()].bvid.clone();
            self.pending_enter = Some((self.playlist_cursor, self.ui.track_list_cursor, bvid));
            self.toggle_expand_current();
            return;
        }
        if self.row_is_expandable(row) {
            self.toggle_expand(row.track_index());
        } else if self.playlist_cursor == 0 {
            match row {
                // A group header stands for its video: play the first part of
                // that video that is actually queued.
                TrackRow::Video(i) | TrackRow::PartGroupHeader(i) => self.play_queue_track(i),
                TrackRow::Page(i, p) => self.play_page_row(i, p),
            }
        } else {
            self.play_playlist_from(row);
        }
    }

    /// Whether Enter should fold this row out into its parts instead of playing
    /// it.
    ///
    /// Only a *video-level* row with several known parts qualifies.  A part row
    /// has no children, and a track that already is a part is a leaf: expanding
    /// it writes to `expanded` but renders nothing (see `visible_rows`), which
    /// made Enter look dead.
    fn row_is_expandable(&self, row: TrackRow) -> bool {
        let TrackRow::Video(i) = row else {
            return false;
        };
        self.active_track_list()
            .get(i)
            .filter(|t| t.page.is_none())
            .and_then(|t| self.pages_cache.get(&t.bvid))
            .map(|pages| pages.len() > 1)
            .unwrap_or(false)
    }

    /// Queue view: jump to the queue track at `i` and play it.
    fn play_queue_track(&mut self, i: usize) {
        if i >= self.queue.len() {
            return;
        }
        self.queue.jump_to(i);

        // Tracks coming from search have cid == 0 and get resolved inside
        // play_track().  If we already have the part list, skip that round-trip.
        let want_cid = match self.queue.current_track() {
            Some(t) if t.cid == 0 => {
                let idx = t.page.map(|p| p.saturating_sub(1) as usize).unwrap_or(0);
                self.pages_cache
                    .get(&t.bvid)
                    .and_then(|pages| pages.get(idx).or_else(|| pages.first()))
                    .map(|p| p.cid)
            }
            _ => None,
        };
        if let (Some(track), Some(cid)) = (self.queue.current_track_mut(), want_cid) {
            track.cid = cid;
        }

        let Some(track) = self.queue.current_track().cloned() else {
            return;
        };
        self.play_track(track);
    }

    /// Queue view: play part `page_idx` of track `track_idx`, enqueueing the
    /// remaining parts of the same video right after it (整集顺序入队) so `n`
    /// walks through them in order.
    fn play_page_row(&mut self, track_idx: usize, page_idx: usize) {
        let Some(base) = self.active_track_list().get(track_idx).cloned() else {
            return;
        };
        let Some(pages) = self.pages_cache.get(&base.bvid).cloned() else {
            return;
        };
        let Some(page) = pages.get(page_idx) else {
            return;
        };
        let part_track = page.to_track(&base);

        // Already queued — jump to it rather than inserting duplicates.
        if let Some(idx) = self
            .queue
            .tracks()
            .iter()
            .position(|t| t.same_item(&part_track))
        {
            let mut previous = idx;
            for page in &pages[page_idx + 1..] {
                let track = page.to_track(&base);
                if let Some(existing) = self.queue.tracks().iter().position(|q| q.same_item(&track))
                {
                    previous = existing;
                } else {
                    previous = self.queue.insert_batch_after(Some(previous), vec![track]);
                }
            }
            let idx = self
                .queue
                .tracks()
                .iter()
                .position(|q| q.same_item(&part_track))
                .unwrap_or(idx);
            self.queue.jump_to(idx);
            if let Some(row) = self.row_of_queue_index(idx) {
                self.ui.track_list_cursor = row;
            }
            let t = self.queue.tracks()[idx].clone();
            self.play_track(t);
            return;
        }

        let pending: Vec<Track> = pages[page_idx..]
            .iter()
            .map(|p| p.to_track(&base))
            .filter(|t| !self.queue.tracks().iter().any(|q| q.same_item(t)))
            .collect();
        if pending.is_empty() {
            return;
        }

        let first = self
            .queue
            .insert_batch_after(self.queue.current_index(), pending);
        self.queue.jump_to(first);
        if let Some(row) = self.row_of_queue_index(first) {
            self.ui.track_list_cursor = row;
        }
        let t = self.queue.tracks()[first].clone();
        self.play_track(t);
        self.set_status(format!("Playing: {}", part_track.display_title()));
    }

    /// Playlist view: replace the queue with the *whole* playlist, multi-part
    /// videos expanded into their individual parts, and start playback on the
    /// row the cursor was on.  Tracks above the cursor stay queued so `p` can
    /// walk back into them.
    fn play_playlist_from(&mut self, row: TrackRow) {
        let pl_idx = self.playlist_cursor - 1;
        let Some(playlist) = self.playlists.get(pl_idx) else {
            return;
        };
        let tracks = playlist.tracks.clone();
        let start = row.track_index();
        if tracks.is_empty() || start >= tracks.len() {
            return;
        }

        let page = match row {
            TrackRow::Page(_, p) => self
                .pages_cache
                .get(&tracks[start].bvid)
                .and_then(|pages| pages.get(p))
                .map(|p| p.page),
            _ => tracks[start].page,
        };
        self.start_queue_plan(
            tracks.clone(),
            start,
            page,
            QueueAction::Replace {
                playlist: pl_idx,
                original: tracks,
            },
        );
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
                self.cover_render_after =
                    Some(std::time::Instant::now() + std::time::Duration::from_millis(120));
            }
            FocusColumn::TrackList => {
                let len = self.visible_row_len();
                if len == 0 {
                    return;
                }
                let cur = self.ui.track_list_cursor as isize;
                let new = (cur + delta).clamp(0, (len - 1) as isize);
                self.ui.track_list_cursor = new as usize;
                // Suppress cover re-render while the user is scrolling quickly.
                // 120 ms after the last j/k the image will appear.
                self.cover_render_after =
                    Some(std::time::Instant::now() + std::time::Duration::from_millis(120));
            }
            _ => {}
        }
    }

    // Popup management
    pub(crate) fn show_popup(&mut self, layer: PopupLayer) {
        self.ui.search_marquee.reset();
        self.force_redraw = true;
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
        self.ui.search_marquee.reset();
        self.force_redraw = true;
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

    /// Slice of tracks for the currently visible middle column.
    pub fn active_track_list(&self) -> &[Track] {
        if self.playlist_cursor == 0 {
            self.queue.tracks()
        } else {
            let pl_idx = self.playlist_cursor - 1;
            self.playlists
                .get(pl_idx)
                .map(|p| p.tracks.as_slice())
                .unwrap_or(&[])
        }
    }

    /// Flattened tree of rows currently shown in the middle column:
    /// one row per track, plus one row per part under each expanded multi-part video.
    ///
    /// `ui.track_list_cursor` is an index into this list, *not* into
    /// `active_track_list()`.
    pub fn visible_rows(&self) -> Vec<TrackRow> {
        let tracks = self.active_track_list();
        let mut rows = Vec::with_capacity(tracks.len());
        let mut previous_bvid: Option<&str> = None;
        for (i, track) in tracks.iter().enumerate() {
            // Label the start of each run of parts with its video.  A part whose
            // video row is already on screen (the parent, expanded) needs no
            // header -- the row above it already carries the title.
            if self.has_visible_part(track) && previous_bvid != Some(track.bvid.as_str()) {
                rows.push(TrackRow::PartGroupHeader(i));
            }
            previous_bvid = Some(track.bvid.as_str());

            rows.push(TrackRow::Video(i));
            // A part-level track (already a 分P) never has children.
            if track.page.is_some() || !self.expanded.contains(&track.bvid) {
                continue;
            }
            if let Some(pages) = self
                .pages_cache
                .get(&track.bvid)
                .filter(|pages| pages.len() > 1)
            {
                rows.extend((0..pages.len()).map(|p| TrackRow::Page(i, p)));
            }
        }
        rows
    }

    fn restore_selected_row(&mut self, selected_row: Option<TrackRow>) {
        if let Some(selected) = selected_row {
            let rows = self.visible_rows();
            let position = rows.iter().position(|row| *row == selected).or_else(|| {
                rows.iter()
                    .position(|row| *row == TrackRow::Video(selected.track_index()))
            });
            if let Some(position) = position {
                self.ui.track_list_cursor = position;
            }
        }
    }

    /// Hide legacy synthetic P1 labels once the source is confirmed single-part.
    /// Playback identity stays intact, including explicitly saved part selections.
    pub fn has_visible_part(&self, track: &Track) -> bool {
        track.page.is_some()
            && !self
                .pages_cache
                .get(&track.bvid)
                .is_some_and(|pages| pages.len() == 1)
    }

    /// Number of rows in the middle column (tracks + expanded parts).
    pub fn visible_row_len(&self) -> usize {
        self.visible_rows().len()
    }

    /// The row under the cursor, if any.
    pub fn current_row(&self) -> Option<TrackRow> {
        self.visible_rows().get(self.ui.track_list_cursor).copied()
    }

    /// The track a row refers to.  Part rows are materialised on the fly from the
    /// cached part list, so they are returned by value.
    pub fn row_track(&self, row: TrackRow) -> Option<Track> {
        let base = self.active_track_list().get(row.track_index())?;
        match row {
            TrackRow::Video(_) => Some(base.clone()),
            TrackRow::Page(_, p) => {
                let page = self.pages_cache.get(&base.bvid)?.get(p)?;
                Some(page.to_track(base))
            }
            // The header stands for the video, not for one part, so drop the
            // part fields and let the inherited title/author show through.
            TrackRow::PartGroupHeader(_) => {
                let mut video = base.clone();
                video.cid = 0;
                video.part = None;
                video.page = None;
                Some(video)
            }
        }
    }

    /// Row position of a queue track, or `None` if it is not in the queue view.
    fn row_of_queue_index(&self, idx: usize) -> Option<usize> {
        if self.playlist_cursor != 0 {
            return None;
        }
        self.visible_rows()
            .iter()
            .position(|r| matches!(r, TrackRow::Video(i) if *i == idx))
    }

    /// Whether a part list request for this bvid is still in flight.
    pub fn is_pages_loading(&self, bvid: &str) -> bool {
        self.pages_loading.contains(bvid)
    }

    /// Part count of a video, if its part list has been fetched.
    pub fn page_count(&self, bvid: &str) -> Option<usize> {
        self.pages_cache.get(bvid).map(|p| p.len())
    }

    /// Expand or collapse the parts of the track row `track_idx`.
    ///
    /// Does nothing when the part list is not loaded yet or the video has a
    /// single part.  Collapsing moves the cursor out of the vanished part rows.
    fn toggle_expand(&mut self, track_idx: usize) {
        let Some(bvid) = self
            .active_track_list()
            .get(track_idx)
            .map(|t| t.bvid.clone())
        else {
            return;
        };
        let Some(pages_len) = self.pages_cache.get(&bvid).map(|p| p.len()) else {
            return;
        };
        if pages_len <= 1 {
            return;
        }

        if self.expanded.contains(&bvid) {
            let cursor_before = self.ui.track_list_cursor;
            let row_before = self.current_row();
            self.expanded.remove(&bvid);

            let parent_row = self
                .visible_rows()
                .iter()
                .position(|r| *r == TrackRow::Video(track_idx))
                .unwrap_or(0);
            match row_before {
                Some(TrackRow::Page(i, _)) if i == track_idx => {
                    // Cursor was inside the children that just disappeared.
                    self.ui.track_list_cursor = parent_row;
                }
                _ if cursor_before > parent_row => {
                    // Rows above the cursor were removed — shift up accordingly.
                    self.ui.track_list_cursor = cursor_before.saturating_sub(pages_len);
                }
                _ => {}
            }
        } else {
            self.expanded.insert(bvid);
        }
    }

    /// Queue a part-list fetch for `bvid` (no-op if already cached or in flight).
    fn request_pages(&mut self, bvid: &str) {
        if bvid.is_empty() || self.pages_cache.contains_key(bvid) {
            return;
        }
        if !self.pages_loading.insert(bvid.to_string()) {
            return;
        }
        let client = self.client.clone();
        let tx = self.pages_tx.clone();
        let bvid = bvid.to_string();
        tokio::spawn(async move {
            let result = get_video_pages(&client, &bvid).await;
            let _ = tx.send((bvid, result));
        });
    }

    /// Background-fetch part lists for the tracks around the cursor.
    ///
    /// Search results carry no part count, so this is what eventually makes the
    /// ▸ marker appear.  Bounded by `MAX_PAGES_IN_FLIGHT` to stay clear of B 站's
    /// rate limiting.
    fn prewarm_pages(&mut self) {
        // The window follows the cursor rather than always covering the head of
        // the list: with a fixed head window, anything past
        // PREFETCH_LOOKAHEAD -- the tail of a long playlist, say -- never got
        // fetched, so it never showed a ▸ marker and Enter played it instead of
        // unfolding it.
        let tracks = self.active_track_list();
        if tracks.is_empty() {
            return;
        }
        let cursor = self
            .current_row()
            .map(|row| row.track_index())
            .unwrap_or(0)
            .min(tracks.len() - 1);
        let end = (cursor + 1 + PREFETCH_LOOKAHEAD / 2).min(tracks.len());
        let start = end.saturating_sub(PREFETCH_LOOKAHEAD);

        // Walk the window starting at the cursor and wrapping round, so the row
        // being looked at is fetched first rather than last.
        let span = end - start;
        let mut requested = Vec::new();
        for offset in 0..span {
            if self.pages_loading.len() + requested.len() >= MAX_PAGES_IN_FLIGHT {
                break;
            }
            let track = &tracks[start + (cursor - start + offset) % span];
            // Saved leaves also need metadata after restart so a single-part
            // video does not retain a legacy synthetic P1 label.
            if track.bvid.is_empty() {
                continue;
            }
            if self
                .pages_retry_after
                .get(&track.bvid)
                .is_some_and(|until| *until > std::time::Instant::now())
                || self.pages_cache.contains_key(&track.bvid)
                || self.pages_loading.contains(&track.bvid)
            {
                continue;
            }
            requested.push(track.bvid.clone());
        }
        for bvid in requested {
            self.request_pages(&bvid);
        }
    }

    /// Toggle whatever row the cursor is on, fetching the part list if needed.
    fn toggle_expand_current(&mut self) {
        let Some(row) = self.current_row() else {
            return;
        };
        let Some(track) = self.active_track_list().get(row.track_index()) else {
            return;
        };
        if track.page.is_some() {
            return;
        }
        let bvid = track.bvid.clone();
        if self.pages_cache.contains_key(&bvid) {
            self.toggle_expand(row.track_index());
        } else {
            // Not loaded yet — fetch it and expand as soon as it arrives.
            self.pending_expand = Some(bvid.clone());
            self.request_pages(&bvid);
        }
    }

    fn save_queue(&mut self) {
        let Some(dir) = &self.persistence_dir else {
            return;
        };
        let (queue_tracks, queue_current_index, play_mode) = self.queue.snapshot();
        let state = AppState {
            queue_tracks,
            queue_current_index,
            play_mode,
        };
        if let Err(e) = state.save_to(&dir.join("state.json")) {
            eprintln!("Failed to save queue: {e}");
            self.set_status(format!("Failed to save queue: {e}"));
        }
    }

    fn save_playlists(&mut self) {
        let Some(dir) = &self.persistence_dir else {
            return;
        };
        let store = PlaylistStore {
            playlists: self.playlists.clone(),
        };
        if let Err(e) = store.save_to(&dir.join("playlists.json")) {
            eprintln!("Failed to save playlists: {e}");
            self.set_status(format!("Failed to save playlists: {e}"));
        }
    }

    /// `a` in the middle column: queue whatever the cursor is on.
    ///
    /// A part row queues just that part; a video row queues the whole video
    /// (every part) once its part list is known.
    fn add_row_to_queue(&mut self, row: TrackRow) {
        let Some(base) = self.active_track_list().get(row.track_index()).cloned() else {
            return;
        };

        let track = match row {
            TrackRow::Page(_, _) => self.row_track(row),
            TrackRow::PartGroupHeader(_) => {
                let mut t = base;
                t.page = None;
                t.part = None;
                t.cid = 0;
                Some(t)
            }
            TrackRow::Video(_) => Some(base),
        };
        if let Some(track) = track {
            self.start_queue_plan(vec![track], 0, None, QueueAction::Append { play: false });
        }
    }

    fn column_visibility(&self) -> (bool, bool) {
        let visibility = crate::ui::layout::ColumnVisibility::from_size(
            self.terminal_width,
            self.terminal_height,
        );
        (visibility.playlist, visibility.detail)
    }

    fn normalize_focus(&mut self) {
        if self.detail_fullscreen_return_focus.is_some() {
            self.focus_column = FocusColumn::Detail;
            return;
        }
        let (playlist, detail) = self.column_visibility();
        if matches!(self.focus_column, FocusColumn::Playlist) && !playlist
            || matches!(self.focus_column, FocusColumn::Detail) && !detail
        {
            self.focus_column = FocusColumn::TrackList;
        }
    }

    fn close_detail_fullscreen(&mut self) {
        if let Some(focus) = self.detail_fullscreen_return_focus.take() {
            self.focus_column = focus;
            self.normalize_focus();
            self.force_redraw = true;
        }
    }

    /// Draw the entire UI. Splits borrows to avoid self conflict.
    pub(crate) fn draw(&mut self, f: &mut ratatui::Frame) {
        let area = f.area();
        self.terminal_width = area.width;
        self.terminal_height = area.height;

        self.normalize_focus();
        let visibility = crate::ui::layout::ColumnVisibility::from_size(area.width, area.height);
        let (header, body, status) = crate::ui::layout::main_layout(area);
        let (playlist, track_list, detail) = crate::ui::layout::body_columns(body, visibility);

        crate::ui::draw_header(f, self, header);
        crate::ui::draw_status_bar(f, self, status);

        let mut cover_manager = self.cover_manager.take();
        if self.detail_fullscreen_return_focus.is_some() {
            crate::ui::now_playing::draw_fullscreen(f, self, &mut cover_manager, body);
        } else {
            crate::ui::playlist_view::draw(f, self, playlist);
            crate::ui::now_playing::draw(f, self, &mut cover_manager, detail);
        }
        self.cover_manager = cover_manager;

        let mut ui = std::mem::take(&mut self.ui);
        if self.detail_fullscreen_return_focus.is_none() {
            crate::ui::track_list::draw(f, self, &mut ui, track_list);
        }

        if self.popup_stack.contains(&PopupLayer::VolumeSlider) {
            crate::ui::volume_slider::draw(f, self, area);
        }
        if self.popup_stack.contains(&PopupLayer::Search) {
            crate::ui::search_view::draw(f, self, &mut ui, area);
        }
        if self.popup_stack.contains(&PopupLayer::Help) {
            crate::ui::help_view::draw(f, self, area);
        }
        if self.popup_stack.contains(&PopupLayer::PlaylistCreate) {
            crate::ui::input_popup::draw(f, self, area);
        }
        if self
            .popup_stack
            .contains(&PopupLayer::PlaylistDeleteConfirm)
        {
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
    use super::*;
    use crate::config::Config;

    fn test_app() -> App {
        App::new_with_playlists(Config::default(), None, Vec::new()).unwrap()
    }

    fn press(app: &mut App, code: KeyCode) {
        app.handle_key(KeyEvent::new(code, crossterm::event::KeyModifiers::NONE));
    }

    #[test]
    fn favorite_input_is_scoped_to_one_open_session() {
        let mut app = test_app();
        app.focus_column = FocusColumn::Playlist;
        app.playlist_name_input = "old playlist name".into();
        press(&mut app, KeyCode::Char('I'));
        assert!(app.playlist_name_input.is_empty());
        for ch in "invalid URL".chars() {
            press(&mut app, KeyCode::Char(ch));
        }
        press(&mut app, KeyCode::Enter);
        assert!(app.is_popup_active(PopupLayer::PlaylistCreate));
        assert_eq!(app.playlist_name_input, "invalid URL");
        press(&mut app, KeyCode::Esc);
        assert!(app.playlist_name_input.is_empty());
        assert!(!app.importing_favorites);
        press(&mut app, KeyCode::Char('I'));
        assert!(app.playlist_name_input.is_empty());
        press(&mut app, KeyCode::Esc);
        press(&mut app, KeyCode::Char('c'));
        assert!(app.playlist_name_input.is_empty());
        assert!(!app.importing_favorites);
    }

    #[tokio::test]
    async fn submitted_favorite_input_is_cleared_after_async_failure_or_cancel() {
        let mut app = test_app();
        app.focus_column = FocusColumn::Playlist;
        press(&mut app, KeyCode::Char('I'));
        for ch in "123".chars() {
            press(&mut app, KeyCode::Char(ch));
        }
        press(&mut app, KeyCode::Enter);
        assert!(app.playlist_name_input.is_empty());
        assert!(!app.is_popup_active(PopupLayer::PlaylistCreate));
        // Replace the spawned request before yielding, avoiding network access.
        let (_, task) = app.pending_favorites.take().unwrap();
        task.abort();
        let (tx, rx) = mpsc::unbounded_channel();
        let task = tokio::spawn(async {});
        app.pending_favorites = Some((rx, task));
        tx.send(Err(crate::error::BilibiliError::Parse(
            "test failure".into(),
        )))
        .unwrap();
        app.poll_pending_work();
        press(&mut app, KeyCode::Char('I'));
        assert!(app.playlist_name_input.is_empty());
        press(&mut app, KeyCode::Esc);
        let (_tx, rx) = mpsc::unbounded_channel();
        app.pending_favorites = Some((rx, tokio::spawn(async {})));
        press(&mut app, KeyCode::Esc);
        assert!(app.pending_favorites.is_none());
        press(&mut app, KeyCode::Char('I'));
        assert!(app.playlist_name_input.is_empty());
    }

    #[tokio::test]
    async fn single_page_metadata_keeps_legacy_leaf_and_later_track_selected() {
        for (cursor, selected, with_later) in [(0, 0, false), (1, 0, false), (2, 1, true)] {
            let mut app = test_app();
            let mut saved = track("legacy");
            saved.page = Some(1);
            saved.part = Some("old P1".into());
            app.queue.push(saved);
            if with_later {
                app.queue.push(track("later"));
            }
            app.ui.track_list_cursor = cursor;
            let before = if cursor == 0 {
                TrackRow::PartGroupHeader(0)
            } else {
                TrackRow::Video(selected)
            };
            assert_eq!(app.current_row(), Some(before));
            app.pages_tx.send(("legacy".into(), Ok(parts(1)))).unwrap();
            app.handle_tick();
            assert_eq!(app.ui.track_list_cursor, selected);
            assert_eq!(app.current_row(), Some(TrackRow::Video(selected)));
            assert_eq!(app.queue.tracks()[0].page, Some(1));
        }
    }

    #[test]
    fn fullscreen_roundtrip_preserves_focus_and_lists_across_resize() {
        let mut app = test_app();
        app.terminal_width = 120;
        app.focus_column = FocusColumn::Playlist;
        app.ui.track_list_cursor = 4;
        app.ui.track_list_scroll = 3;
        app.playlist_cursor = 2;
        app.ui.playlist_list_state.select(Some(2));
        app.ui.playlist_list_state.offset_mut().clone_from(&1);
        press(&mut app, KeyCode::Char('v'));
        assert_eq!(
            app.detail_fullscreen_return_focus,
            Some(FocusColumn::Playlist)
        );
        assert_eq!(app.focus_column, FocusColumn::Detail);
        press(&mut app, KeyCode::Tab);
        press(&mut app, KeyCode::Char('h'));
        press(&mut app, KeyCode::Enter);
        assert_eq!(app.focus_column, FocusColumn::Detail);
        let volume = app.volume;
        press(&mut app, KeyCode::Char('j'));
        assert_eq!(app.volume, volume.saturating_sub(5));
        assert_eq!(app.ui.track_list_cursor, 4);
        assert_eq!(app.ui.track_list_scroll, 3);
        assert_eq!(app.playlist_cursor, 2);
        assert_eq!(app.ui.playlist_list_state.offset(), 1);
        // First Escape dismisses the transient volume overlay.
        press(&mut app, KeyCode::Esc);
        assert!(app.detail_fullscreen_return_focus.is_some());
        app.terminal_width = 60;
        app.normalize_focus();
        assert_eq!(
            app.detail_fullscreen_return_focus,
            Some(FocusColumn::Playlist)
        );
        press(&mut app, KeyCode::Esc);
        assert!(app.detail_fullscreen_return_focus.is_none());
        assert_eq!(app.focus_column, FocusColumn::TrackList);
        assert_eq!(app.ui.track_list_scroll, 3);
    }

    #[test]
    fn fullscreen_modal_keys_and_hidden_columns_do_not_leak() {
        let mut app = test_app();
        app.terminal_width = 60;
        press(&mut app, KeyCode::Char('h'));
        assert_eq!(app.focus_column, FocusColumn::TrackList);
        press(&mut app, KeyCode::Char('l'));
        assert_eq!(app.focus_column, FocusColumn::TrackList);
        press(&mut app, KeyCode::Char('v'));
        press(&mut app, KeyCode::Char('?'));
        press(&mut app, KeyCode::Char('v'));
        assert!(app.detail_fullscreen_return_focus.is_some());
        press(&mut app, KeyCode::Esc);
        assert!(!app.is_popup_active(PopupLayer::Help));
        assert!(app.detail_fullscreen_return_focus.is_some());
        press(&mut app, KeyCode::Char('/'));
        press(&mut app, KeyCode::Char('v'));
        assert_eq!(app.search_query, "v");
        assert!(app.detail_fullscreen_return_focus.is_some());
        press(&mut app, KeyCode::Esc);
        assert_eq!(app.input_mode, InputMode::SearchNormal);
        press(&mut app, KeyCode::Esc);
        assert!(!app.is_popup_active(PopupLayer::Search));
        press(&mut app, KeyCode::Char('v'));
        assert!(app.detail_fullscreen_return_focus.is_none());
        assert_eq!(app.focus_column, FocusColumn::TrackList);
        app.terminal_width = 120;
        app.focus_column = FocusColumn::Detail;
        press(&mut app, KeyCode::Enter);
        assert_eq!(
            app.detail_fullscreen_return_focus,
            Some(FocusColumn::Detail)
        );
        press(&mut app, KeyCode::Esc);
        assert_eq!(app.focus_column, FocusColumn::Detail);
    }

    #[test]
    fn q_unwinds_fullscreen_popups_before_quitting() {
        let mut app = test_app();
        app.terminal_width = 120;
        app.focus_column = FocusColumn::Playlist;
        app.ui.track_list_cursor = 4;
        app.ui.track_list_scroll = 3;
        press(&mut app, KeyCode::Char('v'));
        press(&mut app, KeyCode::Char('m'));
        press(&mut app, KeyCode::Char('q'));
        assert!(!app.is_popup_active(PopupLayer::VolumeSlider));
        assert!(app.detail_fullscreen_return_focus.is_some());
        press(&mut app, KeyCode::Char('?'));
        press(&mut app, KeyCode::Char('q'));
        assert!(!app.is_popup_active(PopupLayer::Help));
        assert!(app.detail_fullscreen_return_focus.is_some());
        press(&mut app, KeyCode::Char('/'));
        press(&mut app, KeyCode::Char('q'));
        assert_eq!(app.search_query, "q");
        assert!(!app.should_quit);
        press(&mut app, KeyCode::Esc);
        press(&mut app, KeyCode::Char('q'));
        assert!(!app.is_popup_active(PopupLayer::Search));
        assert!(app.detail_fullscreen_return_focus.is_some());
        press(&mut app, KeyCode::Char('q'));
        assert!(app.detail_fullscreen_return_focus.is_none());
        assert_eq!(app.focus_column, FocusColumn::Playlist);
        assert_eq!(app.ui.track_list_cursor, 4);
        assert_eq!(app.ui.track_list_scroll, 3);
        assert!(!app.should_quit);
        press(&mut app, KeyCode::Char('q'));
        assert!(app.should_quit);
    }

    #[test]
    fn q_in_playlist_name_is_text_and_main_view_quit_is_preserved() {
        let mut app = test_app();
        app.show_popup(PopupLayer::PlaylistCreate);
        press(&mut app, KeyCode::Char('q'));
        assert!(!app.should_quit);
        assert!(app.is_popup_active(PopupLayer::PlaylistCreate));
        press(&mut app, KeyCode::Esc);
        press(&mut app, KeyCode::Char('q'));
        assert!(app.should_quit);
    }

    // Seed an existing on-disk queue, issue the same delete command as the UI,
    // then reload without run() or its exit handler (as after closing a terminal).
    #[test]
    fn deleted_parts_stay_deleted_without_clean_exit() {
        let dir = tempfile::tempdir().unwrap();
        let mut app = test_app();
        app.persistence_dir = Some(dir.path().to_path_buf());
        app.focus_column = FocusColumn::TrackList;
        for page in 1..=3 {
            let mut part = track("BVparts");
            part.cid = 1000 + page as u64;
            part.page = Some(page);
            part.part = Some(format!("P{page}"));
            app.queue.push(part);
        }
        app.queue.push(track("BVkeep"));
        app.save_queue();
        // The virtual group header occupies row 0; P2 occupies row 2.
        app.ui.track_list_cursor = 2;
        app.handle_command(Command::RemoveFromQueue);
        let state = AppState::load_from(&dir.path().join("state.json")).unwrap();
        assert_eq!(
            state.queue_tracks.iter().map(|t| t.cid).collect::<Vec<_>>(),
            vec![1001, 1003, 12345]
        );
        assert_eq!(state.queue_tracks[1].page, Some(3));
        assert_eq!(state.queue_tracks[2].bvid, "BVkeep");

        // Delete everything, including the final part: an empty snapshot must
        // replace the old non-empty file rather than leave it for next launch.
        while !app.queue.is_empty() {
            app.ui.track_list_cursor = app
                .visible_rows()
                .iter()
                .position(|r| matches!(r, TrackRow::Video(_)))
                .unwrap();
            app.handle_command(Command::RemoveFromQueue);
        }
        let state = AppState::load_from(&dir.path().join("state.json")).unwrap();
        let restored = Queue::restore(
            state.queue_tracks,
            state.queue_current_index,
            state.play_mode,
        );
        assert!(restored.is_empty());
        assert_eq!(restored.current_index(), None);
    }

    #[tokio::test]
    async fn consecutive_playlist_deletions_are_saved_in_order() {
        let dir = tempfile::tempdir().unwrap();
        let mut app = test_app();
        app.persistence_dir = Some(dir.path().to_path_buf());
        app.focus_column = FocusColumn::TrackList;
        app.playlist_cursor = 1;
        let mut playlist = Playlist::new("parts");
        for page in 1..=2 {
            let mut part = track("BVparts");
            part.cid = 1000 + page as u64;
            part.page = Some(page);
            playlist.add_track(part);
        }
        app.playlists.push(playlist);
        app.save_playlists();
        app.ui.track_list_cursor = 1; // first part, below its group header
        app.handle_command(Command::RemoveFromQueue); // routes to playlist deletion
        app.ui.track_list_cursor = 1;
        app.handle_command(Command::RemoveFromPlaylist);
        // Let any accidentally detached old snapshot writers run before reading.
        tokio::task::yield_now().await;
        let store = PlaylistStore::load_from(&dir.path().join("playlists.json")).unwrap();
        assert!(store.playlists[0].tracks.is_empty());
    }

    #[test]
    fn queue_save_failure_is_visible() {
        let dir = tempfile::tempdir().unwrap();
        let blocked = dir.path().join("not-a-directory");
        std::fs::write(&blocked, "blocked").unwrap();
        let mut app = test_app();
        app.persistence_dir = Some(blocked);
        app.focus_column = FocusColumn::TrackList;
        app.queue.push(track("BVdelete"));
        app.handle_command(Command::RemoveFromQueue);
        assert!(
            app.status_message
                .as_deref()
                .unwrap()
                .starts_with("Failed to save queue:")
        );
    }

    fn track(bvid: &str) -> Track {
        Track {
            bvid: bvid.to_string(),
            cid: 12345,
            title: format!("video {bvid}"),
            author: "up".to_string(),
            duration: Duration::from_secs(180),
            cover_url: None,
            part: None,
            page: None,
            source: None,
        }
    }

    fn parts(n: u32) -> Vec<VideoPage> {
        (1..=n)
            .map(|i| VideoPage {
                cid: 1000 + i as u64,
                page: i,
                part: format!("part {i}"),
                duration_secs: 60 + i as u64,
            })
            .collect()
    }

    // ── Multi-part (分P) row model ───────────────────────────────────────────

    #[test]
    fn visible_rows_collapsed_equals_track_count() {
        let mut app = test_app();
        app.queue.push(track("BV1"));
        app.queue.push(track("BV2"));

        assert_eq!(
            app.visible_rows(),
            vec![TrackRow::Video(0), TrackRow::Video(1)]
        );
        assert_eq!(app.visible_row_len(), 2);
    }

    #[test]
    fn visible_rows_expanded_includes_parts() {
        let mut app = test_app();
        let t = track("BV1");
        app.pages_cache.insert(t.bvid.clone(), parts(3));
        app.expanded.insert(t.bvid.clone());
        app.queue.push(t);
        app.queue.push(track("BV2"));

        assert_eq!(
            app.visible_rows(),
            vec![
                TrackRow::Video(0),
                TrackRow::Page(0, 0),
                TrackRow::Page(0, 1),
                TrackRow::Page(0, 2),
                TrackRow::Video(1),
            ]
        );
    }

    #[test]
    fn row_track_materialises_part() {
        let mut app = test_app();
        let t = track("BV1");
        app.pages_cache.insert(t.bvid.clone(), parts(3));
        app.queue.push(t);

        let part = app.row_track(TrackRow::Page(0, 1)).unwrap();
        assert_eq!(part.cid, 1002, "part rows carry their own cid");
        assert_eq!(part.page, Some(2));
        assert_eq!(part.part, Some("part 2".to_string()));
        assert_eq!(part.duration, Duration::from_secs(62));
        assert_eq!(part.bvid, "BV1", "video-level metadata is inherited");
    }

    #[test]
    fn part_level_tracks_are_leaves() {
        let mut app = test_app();
        let mut t = track("BV1");
        t.page = Some(2);
        app.pages_cache.insert(t.bvid.clone(), parts(3));
        app.expanded.insert(t.bvid.clone());
        app.queue.push(t);

        assert_eq!(
            app.visible_rows(),
            vec![TrackRow::PartGroupHeader(0), TrackRow::Video(0)],
            "a track that already is a part must not expand -- it only gets a title row"
        );
    }

    #[test]
    fn consecutive_videos_each_get_their_own_title_row() {
        // Regression: several multi-part videos queued back to back used to
        // blur into one undifferentiated run of `P1 · …` rows.
        let mut app = test_app();
        for bvid in ["BV1", "BV2"] {
            let base = track(bvid);
            app.pages_cache.insert(bvid.to_string(), parts(2));
            for page in app.pages_cache[bvid].clone() {
                app.queue.push(page.to_track(&base));
            }
        }

        assert_eq!(
            app.visible_rows(),
            vec![
                TrackRow::PartGroupHeader(0),
                TrackRow::Video(0),
                TrackRow::Video(1),
                TrackRow::PartGroupHeader(2),
                TrackRow::Video(2),
                TrackRow::Video(3),
            ],
            "each video's run of parts is labelled"
        );
    }

    #[test]
    fn no_title_row_when_the_parent_row_is_already_shown() {
        // An expanded video already shows the title on its own row, so its
        // parts must not get a second one.
        let mut app = test_app();
        let base = track("BV1");
        app.pages_cache.insert(base.bvid.clone(), parts(2));
        app.expanded.insert(base.bvid.clone());
        app.queue.push(base);

        assert_eq!(
            app.visible_rows(),
            vec![
                TrackRow::Video(0),
                TrackRow::Page(0, 0),
                TrackRow::Page(0, 1)
            ]
        );
    }

    #[test]
    fn collapse_moves_cursor_from_part_row_to_parent() {
        let mut app = test_app();
        let t = track("BV1");
        app.pages_cache.insert(t.bvid.clone(), parts(3));
        app.expanded.insert(t.bvid.clone());
        app.queue.push(t);
        app.queue.push(track("BV2"));

        app.ui.track_list_cursor = 2; // on P2, inside the expanded children
        app.toggle_expand(0);

        assert_eq!(
            app.ui.track_list_cursor, 0,
            "cursor lands on the parent row"
        );
        assert_eq!(app.visible_row_len(), 2);
    }

    #[test]
    fn collapse_keeps_cursor_on_the_same_later_track() {
        // Regression guard: collapsing must not drag the cursor onto the parent
        // row when it was sitting on a *later* video.
        let mut app = test_app();
        let t = track("BV1");
        app.pages_cache.insert(t.bvid.clone(), parts(3));
        app.expanded.insert(t.bvid.clone());
        app.queue.push(t);
        app.queue.push(track("BV2"));
        app.queue.push(track("BV3"));

        // rows = [BV1, P1, P2, P3, BV2, BV3]; row 5 is BV3.
        app.ui.track_list_cursor = 5;
        app.toggle_expand(0);

        assert_eq!(
            app.current_row(),
            Some(TrackRow::Video(2)),
            "cursor must stay on BV3, shifted up by the 3 removed rows"
        );
    }

    #[test]
    fn toggle_expand_ignores_single_part_video() {
        let mut app = test_app();
        let t = track("BV1");
        app.pages_cache.insert(t.bvid.clone(), parts(1));
        app.queue.push(t);

        app.toggle_expand(0);
        assert!(
            app.expanded.is_empty(),
            "a 1-part video has nothing to expand"
        );
    }

    // ── Enter behaviour ─────────────────────────────────────────────────────

    #[test]
    fn enter_on_multi_part_queue_row_expands() {
        let mut app = test_app();
        let t = track("BV1");
        app.pages_cache.insert(t.bvid.clone(), parts(3));
        app.queue.push(t);
        app.ui.track_list_cursor = 0;

        app.handle_command(Command::PlaySelected);

        assert!(
            app.expanded.contains("BV1"),
            "Enter expands a multi-part row"
        );
        assert_eq!(app.visible_row_len(), 4);
    }

    #[tokio::test]
    async fn enter_on_queued_part_track_plays_it() {
        // Regression: entering a multi-part video from a playlist fills the
        // queue with part-level tracks.  Those are leaves, so Enter must play
        // them -- it used to expand instead, which rendered nothing and left
        // Enter looking dead.
        let mut app = test_app();
        let base = track("BV1");
        let pages = parts(3);
        app.pages_cache.insert(base.bvid.clone(), pages.clone());
        for page in &pages {
            app.queue.push(page.to_track(&base));
        }
        app.queue.jump_to(0);
        // rows = [title, P1, P2, P3]; row 2 is P2.
        app.ui.track_list_cursor = 2;

        app.handle_command(Command::PlaySelected);

        assert_eq!(
            app.queue.current_index(),
            Some(1),
            "Enter plays the part under the cursor"
        );
        assert!(
            app.expanded.is_empty(),
            "a queued part is a leaf and must not be treated as expandable"
        );
    }

    #[tokio::test]
    async fn enter_on_part_row_enqueues_remaining_parts() {
        let mut app = test_app();
        let t = track("BV1");
        app.pages_cache.insert(t.bvid.clone(), parts(3));
        app.expanded.insert(t.bvid.clone());
        app.queue.push(t);
        app.ui.track_list_cursor = 2; // part index 1 → P2

        app.handle_command(Command::PlaySelected);

        // P2 and P3 are inserted after the current item, in order.
        assert_eq!(app.queue.len(), 3);
        assert_eq!(app.queue.current_index(), Some(1), "playback starts on P2");
        assert_eq!(app.queue.tracks()[1].page, Some(2));
        assert_eq!(app.queue.tracks()[1].cid, 1002);
        assert_eq!(app.queue.tracks()[2].page, Some(3), "P3 follows P2");
    }

    #[tokio::test]
    async fn add_part_row_to_queue_dedups_by_part() {
        let mut app = test_app();
        let t = track("BV1");
        app.pages_cache.insert(t.bvid.clone(), parts(3));
        app.expanded.insert(t.bvid.clone());
        app.playlists.push(Playlist::new("P"));
        app.playlists[0].add_track(t.clone());
        app.playlist_cursor = 1;
        app.focus_column = FocusColumn::TrackList;
        app.ui.track_list_cursor = 2; // P2

        app.handle_command(Command::AddToQueue);
        assert_eq!(app.queue.len(), 1, "only the selected part is queued");
        assert_eq!(app.queue.tracks()[0].cid, 1002);

        // Adding the same part again must not duplicate it.
        app.handle_command(Command::AddToQueue);
        assert_eq!(app.queue.len(), 1);
    }

    #[tokio::test]
    async fn add_video_row_to_queue_takes_every_part() {
        let mut app = test_app();
        let t = track("BV1");
        app.pages_cache.insert(t.bvid.clone(), parts(3));
        app.playlists.push(Playlist::new("P"));
        app.playlists[0].add_track(t.clone());
        app.playlist_cursor = 1;
        app.focus_column = FocusColumn::TrackList;
        app.ui.track_list_cursor = 0; // the video row

        app.handle_command(Command::AddToQueue);

        assert_eq!(app.queue.len(), 3, "a video row queues all of its parts");
        let pages: Vec<u32> = app.queue.tracks().iter().filter_map(|t| t.page).collect();
        assert_eq!(pages, vec![1, 2, 3]);
    }

    #[test]
    fn outgoing_eof_does_not_skip_a_resolving_selection() {
        let mut app = test_app();
        for name in ["A", "B", "C"] {
            app.queue.push(track(name));
        }
        app.queue.jump_to(1);
        let (_tx, rx) = mpsc::unbounded_channel();
        app.pending_stream = Some((app.queue.current_track().unwrap().clone(), rx));
        app.handle_player_event(PlayerEvent::TrackEnded {
            generation: 0,
            reason: EndReason::Eof,
        });
        assert_eq!(app.queue.current_index(), Some(1));
        assert!(
            app.pending_stream
                .as_ref()
                .unwrap()
                .0
                .same_item(app.queue.current_track().unwrap())
        );
    }

    #[test]
    fn deleted_track_resolution_does_not_play_or_update_its_successor() {
        use crate::queue::track::{AudioQuality, TrackSource};
        let mut app = test_app();
        app.queue.push(track("A"));
        app.queue.push(track("B"));
        let (tx, rx) = mpsc::unbounded_channel();
        app.pending_stream = Some((app.queue.current_track().unwrap().clone(), rx));
        app.queue.remove(0);
        tx.send(Ok(TrackSource {
            stream_url: "unused".into(),
            referer: String::new(),
            audio_quality: AudioQuality::LegacyMp4,
        }))
        .unwrap();
        app.handle_tick();
        assert!(app.pending_stream.is_none());
        assert!(app.queue.current_track().unwrap().source.is_none());
        assert!(!app.is_playing);
    }

    #[test]
    fn queued_old_eof_and_error_do_not_change_a_new_load() {
        use crate::queue::track::{AudioQuality, TrackSource};
        let mut app = test_app();
        for name in ["A", "B", "C"] {
            app.queue.push(track(name));
        }
        app.queue.jump_to(1);
        // Simulate B resolution having just completed: no pending_stream remains,
        // but A's already queued events still carry the previous load generation.
        app.player
            .play(&TrackSource {
                stream_url: "invalid\0url".into(),
                referer: String::new(),
                audio_quality: AudioQuality::LegacyMp4,
            })
            .unwrap();
        app.is_playing = true;
        for reason in [EndReason::Eof, EndReason::Error] {
            app.handle_player_event(PlayerEvent::TrackEnded {
                generation: 0,
                reason,
            });
        }
        assert_eq!(app.queue.current_index(), Some(1));
        assert!(app.pending_stream.is_none());
        assert!(app.is_playing);
        assert!(app.status_message.is_none());
    }

    #[test]
    fn sequential_eof_at_tail_stops_without_pending_resolution() {
        let mut app = test_app();
        app.queue.push(track("last"));
        app.is_playing = true;
        app.handle_player_event(PlayerEvent::TrackEnded {
            generation: 0,
            reason: EndReason::Eof,
        });
        assert_eq!(app.queue.current_index(), None);
        assert!(!app.is_playing);
        assert!(app.pending_stream.is_none());
    }

    #[test]
    fn stop_end_reason_is_not_a_playback_error() {
        // Regression: mpv reports the outgoing file as STOP (2) whenever we
        // load another one, so switching tracks used to print
        // "Playback error (reason: 2)" and blank out `is_playing`.
        let mut app = test_app();
        app.queue.push(track("BV1"));
        app.queue.jump_to(0);
        app.is_playing = true;

        app.handle_player_event(PlayerEvent::TrackEnded {
            generation: 0,
            reason: EndReason::Stop,
        });

        assert!(app.is_playing, "replacing the file is not a failure");
        assert_eq!(app.status_message, None, "no bogus error in the status bar");
    }

    #[test]
    fn error_end_reason_stops_playback() {
        let mut app = test_app();
        app.queue.push(track("BV1"));
        app.queue.jump_to(0);
        app.is_playing = true;

        app.handle_player_event(PlayerEvent::TrackEnded {
            generation: 0,
            reason: EndReason::Error,
        });

        assert!(!app.is_playing, "a real playback error stops the UI");
        assert!(app.status_message.is_some());
    }

    #[tokio::test]
    async fn prewarm_covers_tracks_past_the_head_window() {
        // A long playlist: the tail used to sit outside the prefetch window
        // forever, so those videos never became expandable.
        let mut app = test_app();
        app.playlists.push(Playlist::new("P"));
        for i in 0..40 {
            app.playlists[0].add_track(track(&format!("BV{i}")));
        }
        app.playlist_cursor = 1;
        app.focus_column = FocusColumn::TrackList;
        app.ui.track_list_cursor = 39; // the very last entry

        app.prewarm_pages();

        assert!(
            app.is_pages_loading("BV39"),
            "the row under the cursor is fetched first"
        );
    }

    #[test]
    fn enter_on_multi_part_playlist_row_expands() {
        // Enter must fold the parts out first so a specific P can be picked,
        // exactly like it does in the queue view.
        let mut app = test_app();
        let t = track("BV1");
        app.pages_cache.insert(t.bvid.clone(), parts(3));
        app.playlists.push(Playlist::new("P"));
        app.playlists[0].add_track(t);
        app.playlist_cursor = 1;
        app.focus_column = FocusColumn::TrackList;
        app.ui.track_list_cursor = 0;

        app.handle_command(Command::PlaySelected);

        assert!(
            app.expanded.contains("BV1"),
            "Enter folds out the parts instead of playing"
        );
        assert_eq!(app.visible_row_len(), 4);
        assert_eq!(app.queue.len(), 0, "nothing is queued yet");
    }

    #[tokio::test]
    async fn enter_on_playlist_row_keeps_preceding_tracks() {
        // The whole playlist replaces the queue; the cursor only decides where
        // playback starts.  It used to drop everything above the cursor.
        let mut app = test_app();
        app.pages_cache.insert("BV2".into(), parts(1));
        app.pages_cache.insert("BV1".into(), parts(1));
        app.pages_cache.insert("BV3".into(), parts(1));
        app.playlists.push(Playlist::new("P"));
        for bvid in ["BV1", "BV2", "BV3"] {
            app.playlists[0].add_track(track(bvid));
        }
        app.playlist_cursor = 1;
        app.focus_column = FocusColumn::TrackList;
        app.ui.track_list_cursor = 1; // BV2, the middle entry

        app.handle_command(Command::PlaySelected);

        let bvids: Vec<&str> = app.queue.tracks().iter().map(|t| t.bvid.as_str()).collect();
        assert_eq!(
            bvids,
            vec!["BV1", "BV2", "BV3"],
            "the whole playlist replaces the queue"
        );
        assert_eq!(
            app.queue.current_index(),
            Some(1),
            "playback starts on the entered row, earlier tracks stay queued"
        );
    }

    #[tokio::test]
    async fn enter_on_part_row_in_playlist_starts_at_that_part() {
        // Multi-part videos expand into every part, but playback starts on the
        // part the cursor was on.
        let mut app = test_app();
        app.pages_cache.insert("BV2".to_string(), parts(3));
        app.pages_cache.insert("BV1".into(), parts(1));
        app.pages_cache.insert("BV3".into(), parts(1));
        app.playlists.push(Playlist::new("P"));
        for bvid in ["BV1", "BV2", "BV3"] {
            app.playlists[0].add_track(track(bvid));
        }
        app.expanded.insert("BV2".to_string());
        app.playlist_cursor = 1;
        app.focus_column = FocusColumn::TrackList;
        // rows = [BV1, BV2, P1, P2, P3, BV3]; row 3 is P2 of BV2.
        app.ui.track_list_cursor = 3;

        app.handle_command(Command::PlaySelected);

        let bvids: Vec<&str> = app.queue.tracks().iter().map(|t| t.bvid.as_str()).collect();
        assert_eq!(
            bvids,
            vec!["BV1", "BV2", "BV2", "BV2", "BV3"],
            "BV2 expands into all three parts"
        );
        assert_eq!(
            app.queue.current_index(),
            Some(2),
            "playback starts on P2, with BV1 and P1 still ahead of it"
        );
    }

    #[test]
    fn adding_saved_part_never_expands_its_video() {
        let mut app = test_app();
        let base = track("BV1");
        app.pages_cache.insert(base.bvid.clone(), parts(3));
        app.playlists.push(Playlist::new("parts"));
        app.playlists[0].add_track(parts(3)[1].to_track(&base));
        app.playlist_cursor = 1;
        app.add_row_to_queue(TrackRow::Video(0));
        assert_eq!(app.queue.len(), 1);
        assert_eq!(app.queue.tracks()[0].page, Some(2));
    }

    #[tokio::test]
    async fn search_video_starts_at_p1_even_when_p3_is_queued() {
        let mut app = test_app();
        let base = track("BV1");
        app.pages_cache.insert(base.bvid.clone(), parts(3));
        app.queue.push(parts(3)[2].to_track(&base));
        app.start_queue_plan(vec![base], 0, None, QueueAction::Append { play: true });
        assert_eq!(app.queue.len(), 3);
        assert_eq!(app.queue.current_track().unwrap().page, Some(1));
    }

    #[tokio::test]
    async fn cancelled_plan_and_failed_plan_preserve_existing_queue() {
        let mut app = test_app();
        app.queue.push(track("keep"));
        let before = app.queue.snapshot();
        let (tx, rx) = mpsc::unbounded_channel();
        app.pending_queue_plan = Some((
            QueueAction::Append { play: true },
            rx,
            tokio::spawn(async {}),
        ));
        app.cancel_pending_work();
        assert!(
            tx.send(Ok(ResolvedPlan {
                tracks: vec![track("old")],
                start: 0,
                pages: HashMap::new()
            }))
            .is_err()
        );
        let (tx, rx) = mpsc::unbounded_channel();
        app.pending_queue_plan = Some((
            QueueAction::Append { play: true },
            rx,
            tokio::spawn(async {}),
        ));
        tx.send(Err(crate::error::BilibiliError::Parse("offline".into())))
            .ok();
        app.poll_pending_work();
        assert_eq!(before, app.queue.snapshot());
    }

    #[test]
    fn replaced_playlist_source_is_rejected_before_commit() {
        let mut app = test_app();
        app.queue.push(track("keep"));
        app.playlists.push(Playlist::new("source"));
        app.playlists[0].add_track(track("changed"));
        app.playlist_cursor = 1;
        app.commit_queue_plan(
            QueueAction::Replace {
                playlist: 0,
                original: vec![track("original")],
            },
            ResolvedPlan {
                tracks: vec![track("stale")],
                start: 0,
                pages: HashMap::new(),
            },
        );
        assert_eq!(app.queue.tracks()[0].bvid, "keep");
    }

    #[tokio::test]
    async fn async_commit_survives_browsing_and_persists_complete_queue() {
        let mut app = test_app();
        let dir = tempfile::tempdir().unwrap();
        app.persistence_dir = Some(dir.path().to_path_buf());
        let base = track("source");
        app.playlists.push(Playlist::new("source"));
        app.playlists[0].add_track(base.clone());
        let action = QueueAction::Replace {
            playlist: 0,
            original: vec![base.clone()],
        };
        let (tx, rx) = mpsc::unbounded_channel();
        app.pending_queue_plan = Some((action, rx, tokio::spawn(async {})));
        app.handle_command(Command::MoveCursorDown);
        assert!(
            app.pending_queue_plan.is_some(),
            "browsing does not cancel explicit playback intent"
        );
        let tracks = parts(3).iter().map(|p| p.to_track(&base)).collect();
        tx.send(Ok(ResolvedPlan {
            tracks,
            start: 1,
            pages: HashMap::new(),
        }))
        .ok();
        app.poll_pending_work();
        let state = AppState::load_from(&dir.path().join("state.json")).unwrap();
        assert_eq!(state.queue_tracks.len(), 3);
        assert_eq!(state.queue_current_index, Some(1));
        assert_eq!(state.queue_tracks[1].page, Some(2));
    }

    #[tokio::test]
    async fn selecting_existing_part_appends_missing_tail_after_existing_successors() {
        for existing in [vec![0, 1], vec![0, 2]] {
            let mut app = test_app();
            let base = track("BV1");
            app.pages_cache.insert(base.bvid.clone(), parts(3));
            app.expanded.insert(base.bvid.clone());
            app.queue.push(base.clone());
            for page in existing {
                app.queue.push(parts(3)[page].to_track(&base));
            }
            app.play_page_row(0, 0);
            assert_eq!(
                app.queue
                    .tracks()
                    .iter()
                    .filter_map(|t| t.page)
                    .collect::<Vec<_>>(),
                vec![1, 2, 3]
            );
        }
    }

    #[test]
    fn test_extract_bvid_bare() {
        assert_eq!(
            extract_bvid("BV1xx411c7mD"),
            Some("BV1xx411c7mD".to_string())
        );
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

    #[test]
    fn cycling_play_mode_shows_only_icon() {
        let mut app = App::new_with_playlists(Config::default(), None, Vec::new()).unwrap();
        for mode in [
            crate::queue::PlayMode::RepeatList,
            crate::queue::PlayMode::RepeatOne,
            crate::queue::PlayMode::Shuffle,
            crate::queue::PlayMode::Sequential,
        ] {
            app.handle_command(crate::command::Command::CyclePlayMode);
            assert_eq!(app.queue.play_mode, mode);
            assert_eq!(app.status_message.as_deref(), Some(mode.icon()));
        }
    }
}
