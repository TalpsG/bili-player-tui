use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

use crate::app::{FocusColumn, InputMode, PopupLayer};
use crate::command::Command;

/// Convert a key event to a Command based on current mode and focus.
pub fn key_to_command(
    key: KeyEvent,
    mode: InputMode,
    focus: FocusColumn,
    popup_stack: &[PopupLayer],
) -> Command {
    // If Help popup is active, only Esc/? close it
    if popup_stack.contains(&PopupLayer::Help) {
        return help_mode_keys(key);
    }

    // If Search popup is active, handle search input
    if popup_stack.contains(&PopupLayer::Search) {
        return search_mode_keys(key);
    }

    match mode {
        InputMode::Normal => normal_mode_keys(key, focus),
        InputMode::SearchInput => search_mode_keys(key),
    }
}

fn help_mode_keys(key: KeyEvent) -> Command {
    match (key.modifiers, key.code) {
        (KeyModifiers::NONE, KeyCode::Char('?')) => Command::CloseHelp,
        (KeyModifiers::NONE, KeyCode::Esc) => Command::CloseHelp,
        _ => Command::Noop,
    }
}

fn search_mode_keys(key: KeyEvent) -> Command {
    match (key.modifiers, key.code) {
        (KeyModifiers::NONE, KeyCode::Esc) => Command::CloseSearch,
        (KeyModifiers::NONE, KeyCode::Enter) => Command::SearchSubmit,
        (KeyModifiers::NONE, KeyCode::Backspace) => Command::SearchBackspace,
        (KeyModifiers::NONE, KeyCode::Char(c)) => Command::SearchInput(c),
        (KeyModifiers::NONE, KeyCode::Up) | (KeyModifiers::NONE, KeyCode::Char('k')) => Command::MoveCursorUp,
        (KeyModifiers::NONE, KeyCode::Down) | (KeyModifiers::NONE, KeyCode::Char('j')) => Command::MoveCursorDown,
        _ => Command::Noop,
    }
}

fn normal_mode_keys(key: KeyEvent, focus: FocusColumn) -> Command {
    match (key.modifiers, key.code) {
        // Global
        (KeyModifiers::CONTROL, KeyCode::Char('c')) => Command::Quit,
        (KeyModifiers::NONE, KeyCode::Char('q')) => Command::Quit,
        (KeyModifiers::NONE, KeyCode::Char('/')) => Command::OpenSearch,
        (KeyModifiers::NONE, KeyCode::Char('?')) => Command::OpenHelp,
        (KeyModifiers::NONE, KeyCode::Tab) => Command::FocusNext,
        (KeyModifiers::SHIFT, KeyCode::BackTab) => Command::FocusPrev,

        // Playback
        (KeyModifiers::NONE, KeyCode::Char(' ')) => Command::TogglePause,
        (KeyModifiers::NONE, KeyCode::Char('n')) => Command::NextTrack,
        (KeyModifiers::NONE, KeyCode::Char('p')) => Command::PrevTrack,
        (KeyModifiers::NONE, KeyCode::Char('h')) | (KeyModifiers::NONE, KeyCode::Left) => Command::SeekBackward,
        (KeyModifiers::NONE, KeyCode::Char('l')) | (KeyModifiers::NONE, KeyCode::Right) => Command::SeekForward,
        (KeyModifiers::NONE, KeyCode::Char('m')) => Command::ToggleMute,
        (KeyModifiers::NONE, KeyCode::Char('s')) => Command::ToggleShuffle,
        (KeyModifiers::NONE, KeyCode::Char('r')) => Command::CycleRepeat,

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
        (KeyModifiers::NONE, KeyCode::Enter) => Command::PlaySelected,
        (KeyModifiers::NONE, KeyCode::Char('a')) => Command::AddToQueue,
        (KeyModifiers::NONE, KeyCode::Char('d')) => Command::RemoveFromQueue,

        _ => Command::Noop,
    }
}
