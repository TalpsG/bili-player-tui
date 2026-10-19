use ratatui::layout::Rect;
use unicode_width::UnicodeWidthChar;

/// Anchor point for popup positioning.
pub enum Anchor {
    Center,
    AboveStatusBar,
    FullScreen,
}

/// Leave breathing room when the terminal can afford it.
pub fn safe_area(screen: Rect) -> Rect {
    let horizontal = if screen.width > 8 { 2 } else { 0 };
    let vertical = if screen.height > 4 { 1 } else { 0 };
    Rect::new(
        screen.x.saturating_add(horizontal),
        screen.y.saturating_add(vertical),
        screen.width.saturating_sub(horizontal * 2),
        screen.height.saturating_sub(vertical * 2),
    )
}

/// Center a bounded popup inside the screen's safe area.
pub fn popup_area(screen: Rect, anchor: Anchor, width: u16, height: u16) -> Rect {
    if matches!(anchor, Anchor::FullScreen) {
        return screen;
    }
    let available = safe_area(screen);
    let width = width.min(available.width);
    let height = height.min(available.height);
    let x = available
        .x
        .saturating_add(available.width.saturating_sub(width) / 2);
    let y = match anchor {
        Anchor::AboveStatusBar => available
            .y
            .saturating_add(available.height.saturating_sub(height)),
        _ => available
            .y
            .saturating_add(available.height.saturating_sub(height) / 2),
    };
    Rect::new(x, y, width, height)
}

/// Visible input window and cursor column, measured in terminal cells.
/// Reserve a cell for the insertion cursor, including at the end of the input.
pub fn input_window(input: &str, cursor: usize, width: u16) -> (String, u16) {
    if width == 0 {
        return (String::new(), 0);
    }
    let chars: Vec<char> = input.chars().collect();
    let cursor = cursor.min(chars.len());
    let budget = usize::from(width.saturating_sub(1));
    let mut start = cursor;
    let mut before = 0;
    while start > 0 {
        let cells = chars[start - 1].width().unwrap_or(0);
        if before + cells > budget {
            break;
        }
        before += cells;
        start -= 1;
    }
    let mut visible = String::new();
    let mut cells = 0;
    for &ch in &chars[start..] {
        let next = ch.width().unwrap_or(0);
        if cells + next > usize::from(width) {
            break;
        }
        visible.push(ch);
        cells += next;
    }
    (visible, before as u16)
}

#[cfg(test)]
mod tests {
    use super::*;
    use unicode_width::UnicodeWidthStr;

    #[test]
    fn popup_geometry_is_bounded_at_every_size_and_origin() {
        for width in 0..100 {
            for height in 0..40 {
                let screen = Rect::new(11, 7, width, height);
                for anchor in [Anchor::Center, Anchor::AboveStatusBar, Anchor::FullScreen] {
                    let area = popup_area(screen, anchor, u16::MAX, u16::MAX);
                    assert!(area.x >= screen.x && area.y >= screen.y);
                    assert!(area.right() <= screen.right() && area.bottom() <= screen.bottom());
                }
            }
        }
    }

    #[test]
    fn input_window_keeps_unicode_cursor_inside() {
        for input in ["", "abcdef", "音乐abc你好", "a\u{301}中👋xyz"] {
            for width in 0..12 {
                for cursor in 0..=input.chars().count() {
                    let (text, column) = input_window(input, cursor, width);
                    assert!(text.width() <= usize::from(width));
                    assert!(width == 0 || column < width);
                }
            }
        }
    }
    #[test]
    fn overlays_render_inside_tiny_offset_screens() {
        use crate::{
            app::{App, InputMode},
            config::Config,
        };
        use ratatui::{Terminal, backend::TestBackend};
        let mut app = App::new_with_playlists(Config::default(), None, Vec::new()).unwrap();
        app.search_query = "长查询👋音乐".repeat(100);
        app.search_query_cursor = app.search_query.chars().count();
        app.playlist_name_input = "长歌单👋".repeat(100);
        let drawers: [fn(&mut ratatui::Frame, &App, Rect); 6] = [
            |f, app, area| {
                let mut ui = crate::ui::Ui::new();
                super::super::search_view::draw(f, app, &mut ui, area);
            },
            super::super::input_popup::draw,
            super::super::confirm_popup::draw,
            super::super::add_to_playlist_popup::draw,
            super::super::volume_slider::draw,
            super::super::help_view::draw,
        ];
        for mode in [
            InputMode::SearchInput,
            InputMode::SearchNormal,
            InputMode::SearchResults,
        ] {
            app.input_mode = mode;
            for width in 0..15 {
                for height in 0..9 {
                    let screen = Rect::new(3, 2, width, height);
                    for draw in drawers {
                        let mut terminal = Terminal::new(TestBackend::new(20, 14)).unwrap();
                        let frame = terminal.draw(|frame| draw(frame, &app, screen)).unwrap();
                        for y in 0..14 {
                            for x in 0..20 {
                                if x < screen.x
                                    || x >= screen.right()
                                    || y < screen.y
                                    || y >= screen.bottom()
                                {
                                    assert_eq!(frame.buffer[(x, y)].symbol(), " ");
                                }
                            }
                        }
                    }
                }
            }
        }
    }
}
