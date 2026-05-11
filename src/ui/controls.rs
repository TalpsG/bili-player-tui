use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

use crate::app::{FocusColumn, InputMode, PopupLayer};
use crate::command::Command;

/// Convert a key event to a Command based on current mode and focus.
pub fn key_to_command(
    key: KeyEvent,
    mode: InputMode,
    focus: FocusColumn,
    popup_stack: &[PopupLayer],
    search_results_len: usize,
) -> Command {
    // If Help popup is active, only Esc/? close it
    if popup_stack.contains(&PopupLayer::Help) {
        return help_mode_keys(key);
    }

    // AddToPlaylist chooser popup
    if popup_stack.contains(&PopupLayer::AddToPlaylist) {
        return add_to_playlist_mode_keys(key);
    }

    // PlaylistDeleteConfirm popup
    if popup_stack.contains(&PopupLayer::PlaylistDeleteConfirm) {
        return delete_confirm_mode_keys(key);
    }

    // PlaylistCreate input popup
    if popup_stack.contains(&PopupLayer::PlaylistCreate) {
        return create_playlist_mode_keys(key);
    }

    // If Search popup is active, handle search input
    if popup_stack.contains(&PopupLayer::Search) {
        return match mode {
            InputMode::SearchInput => search_input_mode_keys(key),
            InputMode::SearchNormal => search_normal_mode_keys(key, search_results_len),
            _ => Command::Noop,
        };
    }

    match mode {
        InputMode::Normal => normal_mode_keys(key, focus),
        _ => Command::Noop,
    }
}

fn help_mode_keys(key: KeyEvent) -> Command {
    match key.code {
        KeyCode::Char('?') | KeyCode::Esc | KeyCode::Char('q') => Command::CloseHelp,
        _ => Command::Noop,
    }
}

fn create_playlist_mode_keys(key: KeyEvent) -> Command {
    match key.code {
        KeyCode::Esc => Command::CreatePlaylistCancel,
        KeyCode::Enter => Command::CreatePlaylistConfirm,
        KeyCode::Backspace => Command::CreatePlaylistBackspace,
        KeyCode::Char(c) => Command::CreatePlaylistChar(c),
        _ => Command::Noop,
    }
}

fn delete_confirm_mode_keys(key: KeyEvent) -> Command {
    match key.code {
        KeyCode::Enter | KeyCode::Char('y') => Command::DeletePlaylistConfirm,
        KeyCode::Esc | KeyCode::Char('n') | KeyCode::Char('q') => Command::DeletePlaylistCancel,
        _ => Command::Noop,
    }
}

fn add_to_playlist_mode_keys(key: KeyEvent) -> Command {
    match key.code {
        KeyCode::Esc | KeyCode::Char('q') => Command::AddToPlaylistCancel,
        KeyCode::Enter => Command::AddToPlaylistConfirm,
        KeyCode::Char('j') | KeyCode::Down => Command::AddToPlaylistMove(1),
        KeyCode::Char('k') | KeyCode::Up => Command::AddToPlaylistMove(-1),
        _ => Command::Noop,
    }
}

fn search_input_mode_keys(key: KeyEvent) -> Command {
    match key.code {
        KeyCode::Esc => Command::CloseSearch,
        KeyCode::Enter => Command::SearchSubmit,
        KeyCode::Backspace => Command::SearchBackspace,
        KeyCode::Char(c) => Command::SearchInput(c),
        _ => Command::Noop,
    }
}

fn search_normal_mode_keys(key: KeyEvent, _search_results_len: usize) -> Command {
    match (key.modifiers, key.code) {
        (KeyModifiers::NONE, KeyCode::Esc) | (KeyModifiers::NONE, KeyCode::Char('q')) => Command::CloseSearch,
        (KeyModifiers::NONE, KeyCode::Enter) => Command::PlaySelected,
        (KeyModifiers::NONE, KeyCode::Char('i')) => Command::EnterSearchInput,
        (KeyModifiers::NONE, KeyCode::Char('a')) => Command::EnterSearchAppend,
        (KeyModifiers::SHIFT, KeyCode::Char('A')) => Command::AddToPlaylistOpen,
        (KeyModifiers::NONE, KeyCode::Char('h')) | (KeyModifiers::NONE, KeyCode::Left) => Command::MoveCursorLeft,
        (KeyModifiers::NONE, KeyCode::Char('l')) | (KeyModifiers::NONE, KeyCode::Right) => Command::MoveCursorRight,
        (KeyModifiers::NONE, KeyCode::Char('j')) | (KeyModifiers::NONE, KeyCode::Down) => Command::MoveCursorDown,
        (KeyModifiers::NONE, KeyCode::Char('k')) | (KeyModifiers::NONE, KeyCode::Up) => Command::MoveCursorUp,
        _ => Command::Noop,
    }
}

fn normal_mode_keys(key: KeyEvent, focus: FocusColumn) -> Command {
    match (key.modifiers, key.code) {
        // Global
        (KeyModifiers::CONTROL, KeyCode::Char('c')) => Command::Quit,
        (KeyModifiers::NONE, KeyCode::Char('q')) => Command::Quit,
        (KeyModifiers::CONTROL, KeyCode::Char('l')) => Command::Redraw,
        (KeyModifiers::NONE, KeyCode::Char('/')) => Command::OpenSearch,
        (KeyModifiers::NONE, KeyCode::Char('?')) => Command::OpenHelp,
        (KeyModifiers::NONE, KeyCode::Tab) => Command::FocusNext,
        (KeyModifiers::SHIFT, KeyCode::BackTab) => Command::FocusPrev,

        // Playback
        (KeyModifiers::NONE, KeyCode::Char(' ')) => Command::TogglePause,
        (KeyModifiers::NONE, KeyCode::Char('n')) => Command::NextTrack,
        (KeyModifiers::NONE, KeyCode::Char('p')) => Command::PrevTrack,

        // h/l: column navigation (ranger/yazi style)
        (KeyModifiers::NONE, KeyCode::Char('h')) => {
            match focus {
                FocusColumn::Playlist  => Command::Noop,              // already leftmost
                FocusColumn::TrackList => Command::FocusPlaylistColumn,
                FocusColumn::Detail    => Command::FocusTrackListColumn,
            }
        }
        (KeyModifiers::NONE, KeyCode::Char('l')) => {
            match focus {
                FocusColumn::Playlist  => Command::FocusTrackListColumn,
                FocusColumn::TrackList => Command::FocusDetailColumn,
                FocusColumn::Detail    => Command::Noop,              // already rightmost
            }
        }

        // ←/→ arrow keys: always seek (regardless of focus)
        (KeyModifiers::NONE, KeyCode::Left)  => Command::SeekBackward,
        (KeyModifiers::NONE, KeyCode::Right) => Command::SeekForward,

        (KeyModifiers::NONE, KeyCode::Char('m')) => Command::ToggleMute,
        (KeyModifiers::NONE, KeyCode::Char('s')) => Command::Noop,
        (KeyModifiers::NONE, KeyCode::Char('r')) => Command::CyclePlayMode,

        // Arrow keys: cursor or volume depending on focus
        (KeyModifiers::NONE, KeyCode::Up) => {
            match focus {
                FocusColumn::Playlist | FocusColumn::TrackList => Command::MoveCursorUp,
                FocusColumn::Detail => Command::VolumeUp,
            }
        }
        (KeyModifiers::NONE, KeyCode::Down) => {
            match focus {
                FocusColumn::Playlist | FocusColumn::TrackList => Command::MoveCursorDown,
                FocusColumn::Detail => Command::VolumeDown,
            }
        }

        // j/k: cursor movement when focus on list, volume when on detail
        (KeyModifiers::NONE, KeyCode::Char('j')) => {
            match focus {
                FocusColumn::Playlist | FocusColumn::TrackList => Command::MoveCursorDown,
                FocusColumn::Detail => Command::VolumeDown,
            }
        }
        (KeyModifiers::NONE, KeyCode::Char('k')) => {
            match focus {
                FocusColumn::Playlist | FocusColumn::TrackList => Command::MoveCursorUp,
                FocusColumn::Detail => Command::VolumeUp,
            }
        }

        // Page navigation
        (KeyModifiers::SHIFT, KeyCode::Char('J')) | (KeyModifiers::NONE, KeyCode::PageDown) => Command::MoveCursorPageDown,
        (KeyModifiers::SHIFT, KeyCode::Char('K')) | (KeyModifiers::NONE, KeyCode::PageUp) => Command::MoveCursorPageUp,
        (KeyModifiers::NONE, KeyCode::Char('g')) | (KeyModifiers::NONE, KeyCode::Home) => Command::MoveCursorTop,
        (KeyModifiers::SHIFT, KeyCode::Char('G')) | (KeyModifiers::NONE, KeyCode::End) => Command::MoveCursorBottom,

        // Actions
        (KeyModifiers::NONE, KeyCode::Enter) => {
            match focus {
                FocusColumn::Playlist => Command::FocusTrackListColumn,
                _ => Command::PlaySelected,
            }
        }
        (KeyModifiers::NONE, KeyCode::Char('a')) => Command::AddToQueue,
        (KeyModifiers::SHIFT, KeyCode::Char('A')) => Command::AddToPlaylistOpen,
        (KeyModifiers::NONE, KeyCode::Char('c')) => {
            match focus {
                FocusColumn::Playlist => Command::CreatePlaylist,
                _ => Command::Noop,
            }
        }
        (KeyModifiers::NONE, KeyCode::Char('d')) => {
            match focus {
                FocusColumn::Playlist => Command::DeletePlaylist,
                FocusColumn::TrackList => Command::RemoveFromQueue,  // handled contextually in app (queue vs playlist)
                FocusColumn::Detail => Command::Noop,
            }
        }

        _ => Command::Noop,
    }
}
