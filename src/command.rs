/// User-intent commands produced by key binding logic.
/// App::handle_command() dispatches these.
#[derive(Debug, Clone, PartialEq)]
pub enum Command {
    Quit,
    TogglePause,
    NextTrack,
    PrevTrack,
    SeekForward,
    SeekBackward,
    VolumeUp,
    VolumeDown,
    ToggleMute,
    CyclePlayMode,
    FocusNext,
    FocusPrev,
    MoveCursorUp,
    MoveCursorDown,
    MoveCursorPageUp,
    MoveCursorPageDown,
    MoveCursorTop,
    MoveCursorBottom,
    PlaySelected,
    AddToQueue,
    RemoveFromQueue,
    OpenSearch,
    CloseSearch,
    SearchInput(char),
    SearchBackspace,
    SearchSubmit,
    MoveCursorLeft,
    MoveCursorRight,
    OpenHelp,
    CloseHelp,
    EnterSearchInput,
    EnterSearchAppend,
    // Playlist management
    CreatePlaylist,             // 'c' when focus=Playlist → opens input popup
    CreatePlaylistChar(char),   // character typed in create-playlist popup
    CreatePlaylistBackspace,    // Backspace in create-playlist popup
    CreatePlaylistConfirm,      // Enter in create-playlist popup
    CreatePlaylistCancel,       // Esc in create-playlist popup
    DeletePlaylist,             // 'd' when focus=Playlist left column, cursor≠0 → opens confirm popup
    DeletePlaylistConfirm,      // Enter in delete-confirm popup
    DeletePlaylistCancel,       // Esc in delete-confirm popup
    AddToPlaylistOpen,          // 'A' (Shift+A) → opens add-to-playlist chooser popup
    AddToPlaylistMove(isize),   // j/k in add-to-playlist popup
    AddToPlaylistConfirm,       // Enter in add-to-playlist popup
    AddToPlaylistCancel,        // Esc in add-to-playlist popup
    RemoveFromPlaylist,         // 'd' in TrackList while viewing a playlist (removes track from playlist)
    // Column focus
    FocusPlaylistColumn,        // 'h' from TrackList/Detail → go to Playlist column
    FocusTrackListColumn,       // 'h' from Detail / 'l'/Enter from Playlist → go to TrackList column
    FocusDetailColumn,          // 'l' from TrackList → go to Detail column
    /// Ctrl+L: force a full terminal redraw and re-upload all cover images.
    /// Useful when tmux/multiplexer clears graphics data (e.g. window switch).
    Redraw,
    Noop,
}
