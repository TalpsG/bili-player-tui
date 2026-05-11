/// Terminal image protocol detection.
///
/// `Picker::from_termios()` must be called **before** `crossterm::terminal::enable_raw_mode()`
/// because it temporarily writes escape sequences to stdout and reads the terminal response
/// while the terminal is still in cooked (line-buffered) mode.
///
/// Usage from `main.rs`:
///
/// ```ignore
/// let picker = cover::protocol::detect_protocol();
/// let mut app = App::new(config, picker)?;
/// app.run().await?;
/// ```
use ratatui_image::picker::Picker;

/// Attempt to detect the best image protocol for the current terminal.
///
/// Returns `Some(Picker)` on success, `None` if detection fails or the terminal
/// does not support any graphics protocol (in which case we fall back to the
/// ASCII placeholder drawn by `draw_cover_placeholder`).
pub fn detect_protocol() -> Option<Picker> {
    // from_termios queries the terminal for pixel font dimensions and the
    // best supported graphics protocol, all before raw mode is engaged.
    let mut picker = Picker::from_termios().ok()?;

    // Explicitly probe for the best available protocol (Kitty > iTerm2 > Sixel > Halfblocks).
    picker.guess_protocol();

    Some(picker)
}
