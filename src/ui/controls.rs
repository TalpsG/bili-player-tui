use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

use crate::app::{FocusColumn, InputMode, PopupLayer};
use crate::command::Command;

/// Convert a key event to a Command based on current mode and focus.
pub fn key_to_command(
    key: KeyEvent,
    mode: InputMode,
    focus: FocusColumn,
    popup_stack: &[PopupLayer],
    _search_results_len: usize,
) -> Command {
    // Help consumes navigation and close keys before the underlying view.
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
            InputMode::SearchNormal => search_normal_mode_keys(key),
            InputMode::SearchResults => search_results_mode_keys(key),
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
        KeyCode::Char('j') | KeyCode::Down => Command::HelpScroll(1),
        KeyCode::Char('k') | KeyCode::Up => Command::HelpScroll(-1),
        KeyCode::PageDown => Command::HelpPage(1),
        KeyCode::PageUp => Command::HelpPage(-1),
        KeyCode::Home | KeyCode::Char('g') => Command::HelpTop,
        KeyCode::End | KeyCode::Char('G') => Command::HelpBottom,
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
        KeyCode::Esc => Command::SearchEscape,
        KeyCode::Enter => Command::SearchSubmit,
        KeyCode::Backspace => Command::SearchBackspace,
        KeyCode::Left => Command::MoveCursorLeft,
        KeyCode::Right => Command::MoveCursorRight,
        KeyCode::Home => Command::MoveCursorTop,
        KeyCode::End => Command::MoveCursorBottom,
        KeyCode::Char(c)
            if !key
                .modifiers
                .intersects(KeyModifiers::CONTROL | KeyModifiers::ALT | KeyModifiers::SUPER) =>
        {
            Command::SearchInput(c)
        }
        _ => Command::Noop,
    }
}

fn search_normal_mode_keys(key: KeyEvent) -> Command {
    match (key.modifiers, key.code) {
        (_, KeyCode::Esc) => Command::SearchEscape,
        (KeyModifiers::NONE, KeyCode::Char('q')) => Command::CloseSearch,
        (KeyModifiers::NONE, KeyCode::Enter) => Command::SearchSubmit,
        (KeyModifiers::NONE, KeyCode::Char('i') | KeyCode::Char('/')) => Command::EnterSearchInput,
        (KeyModifiers::NONE, KeyCode::Char('a')) => Command::EnterSearchAppend,
        (KeyModifiers::NONE, KeyCode::Char('h') | KeyCode::Left) => Command::MoveCursorLeft,
        (KeyModifiers::NONE, KeyCode::Char('l') | KeyCode::Right) => Command::MoveCursorRight,
        (KeyModifiers::NONE, KeyCode::Char('j') | KeyCode::Down | KeyCode::Tab) => {
            Command::MoveCursorDown
        }
        (KeyModifiers::NONE, KeyCode::Char('k') | KeyCode::Up) => Command::MoveCursorUp,
        (KeyModifiers::NONE, KeyCode::Char('g') | KeyCode::Home) => Command::MoveCursorTop,
        (KeyModifiers::NONE, KeyCode::End) | (KeyModifiers::SHIFT, KeyCode::Char('G')) => {
            Command::MoveCursorBottom
        }
        _ => Command::Noop,
    }
}

fn search_results_mode_keys(key: KeyEvent) -> Command {
    match (key.modifiers, key.code) {
        (_, KeyCode::Esc) => Command::SearchEscape,
        (KeyModifiers::NONE, KeyCode::Char('q')) => Command::CloseSearch,
        (KeyModifiers::NONE, KeyCode::Enter) => Command::PlaySelected,
        (KeyModifiers::NONE, KeyCode::Char('i') | KeyCode::Char('/')) => Command::EnterSearchInput,
        (KeyModifiers::NONE, KeyCode::Char('a')) => Command::AddToQueue,
        (KeyModifiers::NONE | KeyModifiers::SHIFT, KeyCode::Char('A')) => {
            Command::AddToPlaylistOpen
        }
        (KeyModifiers::NONE, KeyCode::Char('j') | KeyCode::Down) => Command::MoveCursorDown,
        (KeyModifiers::NONE, KeyCode::Char('k') | KeyCode::Up) => Command::MoveCursorUp,
        (KeyModifiers::NONE, KeyCode::Tab) | (KeyModifiers::SHIFT, KeyCode::BackTab) => {
            Command::SearchEscape
        }
        (KeyModifiers::NONE, KeyCode::PageDown) | (KeyModifiers::SHIFT, KeyCode::Char('J')) => {
            Command::MoveCursorPageDown
        }
        (KeyModifiers::NONE, KeyCode::PageUp) | (KeyModifiers::SHIFT, KeyCode::Char('K')) => {
            Command::MoveCursorPageUp
        }
        (KeyModifiers::NONE, KeyCode::Char('g') | KeyCode::Home) => Command::MoveCursorTop,
        (KeyModifiers::NONE, KeyCode::End) | (KeyModifiers::SHIFT, KeyCode::Char('G')) => {
            Command::MoveCursorBottom
        }
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

        (KeyModifiers::NONE, KeyCode::Char('v')) => Command::ToggleDetailFullscreen,

        // Playback
        (KeyModifiers::NONE, KeyCode::Char(' ')) => Command::TogglePause,
        (KeyModifiers::NONE, KeyCode::Char('n')) => Command::NextTrack,
        (KeyModifiers::NONE, KeyCode::Char('p')) => Command::PrevTrack,

        // h/l: column navigation (ranger/yazi style)
        (KeyModifiers::NONE, KeyCode::Char('h')) => {
            match focus {
                FocusColumn::Playlist => Command::Noop, // already leftmost
                FocusColumn::TrackList => Command::FocusPlaylistColumn,
                FocusColumn::Detail => Command::FocusTrackListColumn,
            }
        }
        (KeyModifiers::NONE, KeyCode::Char('l')) => {
            match focus {
                FocusColumn::Playlist => Command::FocusTrackListColumn,
                FocusColumn::TrackList => Command::FocusDetailColumn,
                FocusColumn::Detail => Command::Noop, // already rightmost
            }
        }

        // ←/→ arrow keys: always seek (regardless of focus)
        (KeyModifiers::NONE, KeyCode::Left) => Command::SeekBackward,
        (KeyModifiers::NONE, KeyCode::Right) => Command::SeekForward,

        (KeyModifiers::NONE, KeyCode::Char('m')) => Command::ToggleMute,
        (KeyModifiers::NONE, KeyCode::Char('s')) => Command::Noop,
        (KeyModifiers::NONE, KeyCode::Char('r')) => Command::CyclePlayMode,

        // Arrow keys: cursor or volume depending on focus
        (KeyModifiers::NONE, KeyCode::Up) => match focus {
            FocusColumn::Playlist | FocusColumn::TrackList => Command::MoveCursorUp,
            FocusColumn::Detail => Command::VolumeUp,
        },
        (KeyModifiers::NONE, KeyCode::Down) => match focus {
            FocusColumn::Playlist | FocusColumn::TrackList => Command::MoveCursorDown,
            FocusColumn::Detail => Command::VolumeDown,
        },

        // j/k: cursor movement when focus on list, volume when on detail
        (KeyModifiers::NONE, KeyCode::Char('j')) => match focus {
            FocusColumn::Playlist | FocusColumn::TrackList => Command::MoveCursorDown,
            FocusColumn::Detail => Command::VolumeDown,
        },
        (KeyModifiers::NONE, KeyCode::Char('k')) => match focus {
            FocusColumn::Playlist | FocusColumn::TrackList => Command::MoveCursorUp,
            FocusColumn::Detail => Command::VolumeUp,
        },

        // Page navigation
        (KeyModifiers::SHIFT, KeyCode::Char('J')) | (KeyModifiers::NONE, KeyCode::PageDown) => {
            Command::MoveCursorPageDown
        }
        (KeyModifiers::SHIFT, KeyCode::Char('K')) | (KeyModifiers::NONE, KeyCode::PageUp) => {
            Command::MoveCursorPageUp
        }
        (KeyModifiers::NONE, KeyCode::Char('g')) | (KeyModifiers::NONE, KeyCode::Home) => {
            Command::MoveCursorTop
        }
        (KeyModifiers::SHIFT, KeyCode::Char('G')) | (KeyModifiers::NONE, KeyCode::End) => {
            Command::MoveCursorBottom
        }

        // Actions
        (KeyModifiers::NONE, KeyCode::Enter) => match focus {
            FocusColumn::Playlist => Command::FocusTrackListColumn,
            FocusColumn::Detail => Command::ToggleDetailFullscreen,
            FocusColumn::TrackList => Command::PlaySelected,
        },
        // Expand / collapse the parts (分P) of the track under the cursor.
        // Enter already does this for multi-part rows in the queue view, but a
        // playlist row needs a dedicated key because Enter means "play from here".
        (KeyModifiers::NONE, KeyCode::Char('e')) => match focus {
            FocusColumn::TrackList => Command::ToggleExpand,
            _ => Command::Noop,
        },
        (KeyModifiers::NONE, KeyCode::Char('a')) => Command::AddToQueue,
        (KeyModifiers::SHIFT, KeyCode::Char('A')) => Command::AddToPlaylistOpen,
        (_, KeyCode::Char('I')) if focus == FocusColumn::Playlist => Command::ImportFavorites,
        (_, KeyCode::Esc) => Command::CancelPending,
        (KeyModifiers::NONE, KeyCode::Char('c')) => match focus {
            FocusColumn::Playlist => Command::CreatePlaylist,
            _ => Command::Noop,
        },
        (KeyModifiers::NONE, KeyCode::Char('d')) => {
            match focus {
                FocusColumn::Playlist => Command::DeletePlaylist,
                FocusColumn::TrackList => Command::RemoveFromQueue, // handled contextually in app (queue vs playlist)
                FocusColumn::Detail => Command::Noop,
            }
        }

        _ => Command::Noop,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn search_key(mode: InputMode, code: KeyCode) -> Command {
        key_to_command(
            KeyEvent::new(code, KeyModifiers::NONE),
            mode,
            FocusColumn::TrackList,
            &[PopupLayer::Search],
            3,
        )
    }

    #[test]
    fn escape_unwinds_insert_and_results_before_closing() {
        for start in [InputMode::SearchInput, InputMode::SearchResults] {
            assert_eq!(search_key(start, KeyCode::Esc), Command::SearchEscape);
            let normal = start.search_escape();
            assert_eq!(normal, InputMode::SearchNormal);
            assert!(normal.search_focus_input());
            assert_eq!(search_key(normal, KeyCode::Esc), Command::SearchEscape);
            assert_eq!(normal.search_escape(), InputMode::Normal);
        }
    }

    #[test]
    fn complete_submit_escape_escape_path_restores_global_mode() {
        let insert = InputMode::SearchInput;
        assert_eq!(search_key(insert, KeyCode::Enter), Command::SearchSubmit);
        let results = insert.search_submitted("query");
        assert_eq!(search_key(results, KeyCode::Esc), Command::SearchEscape);
        let query_normal = results.search_escape();
        assert_eq!(query_normal, InputMode::SearchNormal);
        // Completion between the two Esc presses must not re-enter results.
        let completed = query_normal.search_finished(true);
        assert_eq!(search_key(completed, KeyCode::Esc), Command::SearchEscape);
        let closed = completed.search_escape();
        assert_eq!(closed, InputMode::Normal);
        assert_eq!(closed.search_finished(true), InputMode::Normal);
    }

    #[test]
    fn submit_focuses_results_before_response_and_empty_query_stays_put() {
        for mode in [InputMode::SearchInput, InputMode::SearchNormal] {
            assert_eq!(search_key(mode, KeyCode::Enter), Command::SearchSubmit);
            let submitted = mode.search_submitted("音乐");
            assert_eq!(submitted, InputMode::SearchResults);
            assert!(!submitted.search_focus_input());
            assert_eq!(submitted.search_finished(true), InputMode::SearchResults);
            assert_eq!(mode.search_submitted("  "), mode);
        }
        assert_eq!(
            search_key(InputMode::SearchResults, KeyCode::Enter),
            Command::PlaySelected
        );
    }

    #[test]
    fn late_response_preserves_escape_edit_and_closed_modes() {
        let submitted = InputMode::SearchInput.search_submitted("query");
        let normal = submitted.search_escape();
        for mode in [normal, InputMode::SearchInput, normal.search_escape()] {
            for has_results in [true, false] {
                assert_eq!(mode.search_finished(has_results), mode);
            }
        }
        assert_eq!(submitted.search_finished(false), InputMode::SearchNormal);
    }

    #[test]
    fn result_actions_do_not_leak_into_query_modes() {
        for (mode, expected) in [
            (InputMode::SearchInput, Command::SearchInput('a')),
            (InputMode::SearchNormal, Command::EnterSearchAppend),
            (InputMode::SearchResults, Command::AddToQueue),
        ] {
            assert_eq!(search_key(mode, KeyCode::Char('a')), expected);
        }
        assert_eq!(
            search_key(InputMode::SearchNormal, KeyCode::Char('A')),
            Command::Noop
        );
        for modifiers in [KeyModifiers::NONE, KeyModifiers::SHIFT] {
            assert_eq!(
                key_to_command(
                    KeyEvent::new(KeyCode::Char('A'), modifiers),
                    InputMode::SearchResults,
                    FocusColumn::TrackList,
                    &[PopupLayer::Search],
                    3,
                ),
                Command::AddToPlaylistOpen
            );
        }
    }

    #[test]
    fn insert_keeps_vim_characters_literal_and_supports_cursor_arrows() {
        for c in ['i', 'j', 'k', 'q', '/', '中'] {
            assert_eq!(
                search_key(InputMode::SearchInput, KeyCode::Char(c)),
                Command::SearchInput(c)
            );
        }
        for (key, command) in [
            (KeyCode::Left, Command::MoveCursorLeft),
            (KeyCode::Right, Command::MoveCursorRight),
            (KeyCode::Home, Command::MoveCursorTop),
            (KeyCode::End, Command::MoveCursorBottom),
        ] {
            assert_eq!(search_key(InputMode::SearchInput, key), command);
        }
        assert_eq!(
            key_to_command(
                KeyEvent::new(KeyCode::Char('a'), KeyModifiers::CONTROL),
                InputMode::SearchInput,
                FocusColumn::TrackList,
                &[PopupLayer::Search],
                0,
            ),
            Command::Noop
        );
    }

    #[test]
    fn query_normal_and_results_have_distinct_navigation() {
        for mode in [InputMode::SearchNormal, InputMode::SearchResults] {
            assert_eq!(
                search_key(mode, KeyCode::Char('i')),
                Command::EnterSearchInput
            );
            assert_eq!(
                search_key(mode, KeyCode::Char('/')),
                Command::EnterSearchInput
            );
            assert_eq!(
                search_key(mode, KeyCode::Char('j')),
                Command::MoveCursorDown
            );
            assert_eq!(search_key(mode, KeyCode::Char('k')), Command::MoveCursorUp);
        }
        assert_eq!(
            search_key(InputMode::SearchNormal, KeyCode::Left),
            Command::MoveCursorLeft
        );
        assert_eq!(
            search_key(InputMode::SearchResults, KeyCode::Left),
            Command::Noop
        );
        assert_eq!(
            search_key(InputMode::SearchResults, KeyCode::PageDown),
            Command::MoveCursorPageDown
        );
        assert_eq!(
            search_key(InputMode::SearchNormal, KeyCode::Tab),
            Command::MoveCursorDown
        );
        assert_eq!(
            search_key(InputMode::SearchResults, KeyCode::Tab),
            Command::SearchEscape
        );
    }

    #[test]
    fn nested_popup_escape_does_not_unwind_search() {
        for (popup, expected) in [
            (PopupLayer::Help, Command::CloseHelp),
            (PopupLayer::AddToPlaylist, Command::AddToPlaylistCancel),
            (PopupLayer::PlaylistCreate, Command::CreatePlaylistCancel),
            (
                PopupLayer::PlaylistDeleteConfirm,
                Command::DeletePlaylistCancel,
            ),
        ] {
            assert_eq!(
                key_to_command(
                    KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE),
                    InputMode::SearchResults,
                    FocusColumn::TrackList,
                    &[PopupLayer::Search, popup],
                    3,
                ),
                expected
            );
        }
    }

    #[test]
    fn closed_search_routes_global_keys_again() {
        assert_eq!(
            key_to_command(
                KeyEvent::new(KeyCode::Char('/'), KeyModifiers::NONE),
                InputMode::SearchNormal.search_escape(),
                FocusColumn::TrackList,
                &[],
                0,
            ),
            Command::OpenSearch
        );
    }
}
