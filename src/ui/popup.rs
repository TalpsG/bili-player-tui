use ratatui::layout::Rect;

/// Anchor point for popup positioning.
pub enum Anchor {
    /// Centered in the screen
    Center,
    /// Centered horizontally, positioned above the status bar
    AboveStatusBar,
    /// Full screen
    FullScreen,
}

/// Calculate popup area given anchor and desired size.
pub fn popup_area(screen: Rect, anchor: Anchor, width: u16, height: u16) -> Rect {
    let (x, y) = match anchor {
        Anchor::Center => {
            let x = screen.width.saturating_sub(width) / 2;
            let y = screen.height.saturating_sub(height) / 2;
            (x, y)
        }
        Anchor::AboveStatusBar => {
            let x = screen.width.saturating_sub(width) / 2;
            let y = screen.height.saturating_sub(height + 1); // +1 for status bar
            (x, y)
        }
        Anchor::FullScreen => (0, 0),
    };

    Rect::new(
        x,
        y,
        width.min(screen.width),
        height.min(screen.height),
    )
}
