use std::time::Duration;

use crossterm::{
    event::{Event as CrosstermEvent, KeyCode, KeyEvent, KeyModifiers},
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
use crate::event::PlayerEvent;
use crate::player::mpv::MpvBackend;
use crate::queue::Queue;
use crate::queue::track::Track;
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
}

impl App {
    pub fn new(config: Config) -> anyhow::Result<Self> {
        let logged_in = !config.bilibili.sessdata.is_empty();
        let client = BilibiliClient::new(Some(config.bilibili.sessdata.clone()));
        let mut player = MpvBackend::new()?;
        let volume = config.player.volume;
        player.set_volume(volume);

        Ok(Self {
            config,
            client,
            player,
            queue: Queue::new(),
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
        })
    }

    /// Run the TUI main loop.
    pub async fn run(&mut self) -> anyhow::Result<()> {
        // Setup terminal
        enable_raw_mode()?;
        let mut stdout = std::io::stdout();
        execute!(stdout, EnterAlternateScreen)?;
        let backend = CrosstermBackend::new(stdout);
        let mut terminal = Terminal::new(backend)?;
        terminal.clear()?;

        // Set up panic hook to restore terminal
        let panic_hook = std::panic::take_hook();
        std::panic::set_hook(Box::new(move |info| {
            let _ = disable_raw_mode();
            let _ = execute!(std::io::stdout(), LeaveAlternateScreen);
            panic_hook(info);
        }));

        // Event channels
        let (key_tx, mut key_rx) = mpsc::unbounded_channel();

        // Spawn crossterm event reader task
        tokio::task::spawn_blocking(move || {
            loop {
                if let Ok(CrosstermEvent::Key(key)) = crossterm::event::read() {
                    if key_tx.send(key).is_err() {
                        break;
                    }
                }
            }
        });

        // Tick interval (250ms for 4 FPS)
        let mut tick_interval = tokio::time::interval(Duration::from_millis(250));

        let result = self.run_loop(&mut terminal, &mut key_rx, &mut tick_interval).await;

        // Restore terminal
        disable_raw_mode()?;
        execute!(terminal.backend_mut(), LeaveAlternateScreen)?;

        let _ = self.player.shutdown();

        result
    }

    async fn run_loop(
        &mut self,
        terminal: &mut Terminal<CrosstermBackend<std::io::Stdout>>,
        key_rx: &mut mpsc::UnboundedReceiver<KeyEvent>,
        tick_interval: &mut tokio::time::Interval,
    ) -> anyhow::Result<()> {
        loop {
            if self.should_quit {
                break;
            }

            tokio::select! {
                // Key events from crossterm
                key = key_rx.recv() => {
                    if let Some(key) = key {
                        self.handle_key(key);
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

            // Render
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
                if let Some(track) = self.queue.next() {
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
            Command::ToggleShuffle => {
                // P2: no-op in P1
                self.set_status("Shuffle: P2 feature".to_string());
            }
            Command::CycleRepeat => {
                // P2: no-op in P1
                self.set_status("Repeat: P2 feature".to_string());
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
                if self.popup_stack.contains(&PopupLayer::Search) && self.search_focus_input {
                    if self.search_query_cursor > 0 {
                        self.search_query_cursor -= 1;
                    }
                }
            }
            Command::MoveCursorRight => {
                if self.popup_stack.contains(&PopupLayer::Search) && self.search_focus_input {
                    if self.search_query_cursor < self.search_query.chars().count() {
                        self.search_query_cursor += 1;
                    }
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
                        FocusColumn::TrackList => {
                            if !self.queue.is_empty() {
                                self.ui.track_list_cursor = 0;
                            }
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
                        FocusColumn::TrackList => {
                            if !self.queue.is_empty() {
                                self.ui.track_list_cursor = self.queue.len() - 1;
                            }
                        }
                        _ => {}
                    }
                }
            }
            Command::PlaySelected => {
                if self.popup_stack.contains(&PopupLayer::Search) {
                    if self.search_focus_input {
                        // Enter in input field (Normal mode) re-submits search
                        self.handle_command(Command::SearchSubmit);
                    } else if !self.search_results.is_empty() && self.search_cursor < self.search_results.len() {
                        // Play selected search result
                        let track = self.search_results[self.search_cursor].clone();
                        self.queue.push(track.clone());
                        let idx = self.queue.len() - 1;
                        self.queue.jump_to(idx);
                        self.play_track(track);
                        self.set_status(format!("Playing: {}", self.search_results[self.search_cursor].title));
                    }
                } else {
                    self.play_selected();
                }
            }
            Command::AddToQueue => {
                if self.popup_stack.contains(&PopupLayer::Search) {
                    if !self.search_results.is_empty() && self.search_cursor < self.search_results.len() {
                        let track = self.search_results[self.search_cursor].clone();
                        self.queue.push(track);
                        self.set_status("Added to queue".to_string());
                    }
                }
            }
            Command::RemoveFromQueue => {
                if self.focus_column == FocusColumn::TrackList && !self.queue.is_empty() {
                    let idx = self.ui.track_list_cursor;
                    if idx < self.queue.len() {
                        self.queue.remove(idx);
                        if self.ui.track_list_cursor > 0 && self.ui.track_list_cursor >= self.queue.len() {
                            self.ui.track_list_cursor = self.queue.len().saturating_sub(1);
                        }
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
            Command::OpenHelp => {
                self.show_popup(PopupLayer::Help);
            }
            Command::CloseHelp => {
                self.hide_popup(PopupLayer::Help);
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
        if self.popup_stack.contains(&PopupLayer::VolumeSlider) {
            if let Some(ts) = self.volume_popup_time {
                if ts.elapsed() > Duration::from_secs(3) {
                    self.popup_stack.retain(|&l| l != PopupLayer::VolumeSlider);
                    self.volume_popup_time = None;
                }
            }
        }
    }

    fn handle_player_event(&mut self, event: PlayerEvent) {
        match event {
            PlayerEvent::TrackEnded { reason } => {
                if reason == 0 {
                    if let Some(track) = self.queue.next() {
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
        match self.focus_column {
            FocusColumn::TrackList => {
                if self.ui.track_list_cursor < self.queue.len() {
                    let idx = self.ui.track_list_cursor;
                    self.queue.jump_to(idx);
                    if let Some(track) = self.queue.current_track() {
                        let track = track.clone();
                        self.play_track(track);
                    }
                }
            }
            _ => {}
        }
    }

    fn move_cursor(&mut self, delta: isize) {
        match self.focus_column {
            FocusColumn::TrackList => {
                if self.queue.is_empty() {
                    return;
                }
                let cur = self.ui.track_list_cursor as isize;
                let new = (cur + delta).clamp(0, (self.queue.len() - 1) as isize);
                self.ui.track_list_cursor = new as usize;
            }
            _ => {}
        }
    }

    // Popup management
    fn show_popup(&mut self, layer: PopupLayer) {
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
    fn draw(&mut self, f: &mut ratatui::Frame) {
        let area = f.area();
        self.terminal_width = area.width;
        self.terminal_height = area.height;

        let visibility = crate::ui::layout::ColumnVisibility::from_width(area.width);
        let (header, body, status) = crate::ui::layout::main_layout(area);
        let (playlist, track_list, detail) = crate::ui::layout::body_columns(body, visibility);

        crate::ui::draw_header(f, self, header);
        crate::ui::draw_status_bar(f, self, status);

        crate::ui::playlist_view::draw(f, self, playlist);
        crate::ui::now_playing::draw(f, self, detail);

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

        self.ui = ui;
    }
}

impl Default for App {
    fn default() -> Self {
        Self::new(Config::default()).unwrap()
    }
}
