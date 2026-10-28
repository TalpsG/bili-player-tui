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
#[cfg(unix)]
pub fn detect_protocol() -> Option<Picker> {
    // from_termios queries the terminal for pixel font dimensions and the
    // best supported graphics protocol, all before raw mode is engaged.
    let mut picker = Picker::from_termios().ok()?;

    // Explicitly probe for the best available protocol (Kitty > iTerm2 > Sixel > Halfblocks).
    picker.guess_protocol();

    Some(picker)
}

/// Windows has no termios font-size query. Halfblocks works without a terminal
/// graphics extension; the cell size controls the image's aspect ratio.
#[cfg(not(unix))]
pub fn detect_protocol() -> Option<Picker> {
    let cell_size = std::env::var("BILI_PLAYER_CELL_SIZE")
        .ok()
        .and_then(|value| parse_cell_size(&value))
        .unwrap_or((8, 16));
    Some(Picker::new(cell_size))
}

#[cfg(any(not(unix), test))]
fn parse_cell_size(value: &str) -> Option<(u16, u16)> {
    let (width, height) = value.split_once('x')?;
    let size = (width.parse().ok()?, height.parse().ok()?);
    (size.0 > 0 && size.1 > 0).then_some(size)
}

#[cfg(test)]
mod tests {
    use super::parse_cell_size;

    #[test]
    fn cell_dimensions_must_be_positive_and_fit_u16() {
        assert_eq!(parse_cell_size("9x18"), Some((9, 18)));
        for value in ["0x16", "8x0", "65536x16", "8", "8x16x2", "-8x16"] {
            assert_eq!(parse_cell_size(value), None);
        }
    }
}
