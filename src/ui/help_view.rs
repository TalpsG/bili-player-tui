use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::style::{Color, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Clear, Paragraph};
use unicode_width::UnicodeWidthChar;

use crate::app::App;

fn area(screen: Rect) -> Rect {
    super::popup::popup_area(
        screen,
        super::popup::Anchor::Center,
        ((u32::from(screen.width) * 4 / 5) as u16).clamp(50, 110),
        ((u32::from(screen.height) * 4 / 5) as u16).max(3),
    )
}

fn body_area(screen: Rect) -> Rect {
    Block::default().borders(Borders::ALL).inner(area(screen))
}

pub fn page_height(screen: Rect) -> usize {
    usize::from(body_area(screen).height).max(1)
}

pub fn scroll_limit(screen: Rect) -> usize {
    help_lines(body_area(screen).width)
        .len()
        .saturating_sub(page_height(screen))
}

/// Wrap before rendering so every visual line is reachable through scrolling.
fn help_lines(width: u16) -> Vec<Line<'static>> {
    let keybindings: &[(&str, &str)] = &[
        (
            "h/j/k/l",
            "Vim-like navigation; Tab / Shift+Tab switch columns",
        ),
        ("Enter", "Select or confirm"),
        (
            "q / Esc",
            "Back or close; main-screen q quits (outside text input)",
        ),
        ("/", "Search; i edits, Esc returns to Vim-like navigation"),
        ("Space", "Play / pause"),
        ("n / p", "Next / previous track"),
        ("v", "Full-screen details"),
        ("r", "Cycle playback mode"),
        ("I", "Import favorites (playlist column)"),
        ("?", "Help; see KEYBINDINGS.md for all shortcuts"),
    ];

    let mut lines = Vec::new();
    let width = usize::from(width);
    if width == 0 {
        return lines;
    }
    for &(key, desc) in keybindings {
        // Keep each shortcut on one line when possible, wrapping on small screens.
        let entry = format!("{key}: {desc}");
        for (text, style) in [(entry.as_str(), Style::default().fg(Color::Cyan))] {
            let mut current = String::new();
            let mut cells = 0;
            for ch in text.chars() {
                let char_width = ch.width().unwrap_or(0);
                if cells + char_width > width && !current.is_empty() {
                    lines.push(Line::from(Span::styled(
                        std::mem::take(&mut current),
                        style,
                    )));
                    cells = 0;
                }
                if char_width <= width {
                    current.push(ch);
                    cells += char_width;
                } else {
                    // No complete wide glyph fits in a one-column viewport.
                    current.push('�');
                    cells += 1;
                }
            }
            if !current.is_empty() {
                lines.push(Line::from(Span::styled(current, style)));
            }
        }
    }
    lines
}

/// Draw the scrollable keybinding reference.
pub fn draw(f: &mut Frame, app: &App, screen: Rect) {
    let area = area(screen);
    let inner = body_area(screen);
    let lines = help_lines(inner.width);
    let offset = app
        .help_scroll
        .min(lines.len().saturating_sub(usize::from(inner.height)));
    let visible = lines
        .into_iter()
        .skip(offset)
        .take(usize::from(inner.height))
        .collect::<Vec<_>>();
    let block = Block::default()
        .borders(Borders::ALL)
        .title(" Help (q / ? / Esc close) ")
        .title_bottom(" j/k scroll · PgUp/PgDn · Home/End ")
        .style(app.ui.theme.popup_border);
    f.render_widget(Clear, area);
    f.render_widget(block, area);
    f.render_widget(Paragraph::new(visible), inner);
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn help_wraps_without_losing_final_entry() {
        for width in 1..100 {
            let lines = help_lines(width);
            assert!(lines.iter().all(|line| line.width() <= usize::from(width)));
            let text = lines
                .iter()
                .map(|line| line.to_string())
                .collect::<String>();
            if width >= 2 {
                assert!(text.contains("Help; see KEYBINDINGS.md for all shortcuts"));
            }
        }
    }
    #[test]
    fn help_scroll_exposes_last_line_on_small_screens() {
        let screen = Rect::new(9, 4, 24, 10);
        assert_eq!(
            scroll_limit(screen) + page_height(screen),
            help_lines(body_area(screen).width).len()
        );
    }
}
