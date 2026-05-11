use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Clear, Paragraph};
use ratatui::Frame;

use crate::app::App;

/// Draw the "Create Playlist" input popup.
///
/// Layout: centered box with a title, an input line, and a hint footer.
pub fn draw(f: &mut Frame, app: &App, screen: Rect) {
    let width = 50u16.min(screen.width);
    let height = 5u16;

    let area = super::popup::popup_area(
        screen,
        super::popup::Anchor::Center,
        width,
        height,
    );

    // Clear the background
    f.render_widget(Clear, area);

    let block = Block::default()
        .borders(Borders::ALL)
        .title(" New Playlist ")
        .border_style(app.ui.theme.popup_border);

    // Split into: input line (row 0) + hint line (row 1) inside the block
    let inner = block.inner(area);
    f.render_widget(block, area);

    let rows = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Length(1), Constraint::Length(1)])
        .split(inner);

    // Input line: show text with trailing cursor block
    let input = &app.playlist_name_input;
    let input_line = Line::from(vec![
        Span::raw(" "),
        Span::styled(input.as_str(), Style::default().fg(Color::White).add_modifier(Modifier::BOLD)),
        Span::styled("█", Style::default().fg(Color::White)),
    ]);
    f.render_widget(Paragraph::new(input_line), rows[0]);

    // Hint line
    let hint = Line::from(vec![
        Span::styled(" Enter", Style::default().fg(Color::Green)),
        Span::raw(" confirm  "),
        Span::styled("Esc", Style::default().fg(Color::Red)),
        Span::raw(" cancel"),
    ]);
    f.render_widget(Paragraph::new(hint), rows[1]);
}
