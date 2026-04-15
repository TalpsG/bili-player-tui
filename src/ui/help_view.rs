use ratatui::layout::Rect;
use ratatui::style::{Color, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Clear, List, ListItem};
use ratatui::Frame;

use crate::app::App;

/// Draw help overlay: centered popup with keybinding reference.
pub fn draw(f: &mut Frame, app: &App, screen: Rect) {
    // Determine size (80% of screen but not too small)
    let width = (screen.width as f32 * 0.8) as u16;
    let height = (screen.height as f32 * 0.8) as u16;
    
    let area = super::popup::popup_area(
        screen,
        super::popup::Anchor::Center,
        width.max(50),
        height.max(15),
    );

    let keybindings: &[(&str, &str)] = &[
        ("q / Ctrl+C", "Quit"),
        ("/", "Search"),
        ("?", "Help (this screen)"),
        ("Tab / Shift+Tab", "Switch focus"),
        ("Space", "Pause/Resume"),
        ("n / p", "Next/Previous track"),
        ("h / l / ← / →", "Seek -5s / +5s"),
        ("↑ / ↓ (detail focus)", "Volume +5 / -5"),
        ("j / k (list focus)", "Cursor down / up"),
        ("J / K / PgDn / PgUp", "Page down / up"),
        ("g / G / Home / End", "Jump to top / bottom"),
        ("Enter", "Play selected"),
        ("a", "Add to queue"),
        ("d", "Remove from queue"),
        ("m", "Mute toggle"),
        ("s", "Shuffle (P2)"),
        ("r", "Repeat cycle (P2)"),
    ];

    let items: Vec<ListItem> = keybindings
        .iter()
        .map(|&(key, desc)| {
            let line = Line::from(vec![
                Span::styled(
                    format!("  {key:<28}"),
                    Style::default().fg(Color::Cyan),
                ),
                Span::raw(desc),
            ]);
            ListItem::new(line)
        })
        .collect();

    let block = Block::default()
        .borders(Borders::ALL)
        .title(" Keybindings (press q, ? or Esc to close) ")
        .style(app.ui.theme.popup_border);

    let list = List::new(items).block(block);

    // Clear background to avoid overlapping lower level content
    f.render_widget(Clear, area);
    f.render_widget(list, area);
}
