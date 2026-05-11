use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::style::{Color, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Clear, Paragraph};
use ratatui::Frame;

use crate::app::App;

/// Draw the "Delete Playlist" confirmation popup.
pub fn draw(f: &mut Frame, app: &App, screen: Rect) {
    let width = 52u16.min(screen.width);
    let height = 5u16;

    let area = super::popup::popup_area(
        screen,
        super::popup::Anchor::Center,
        width,
        height,
    );

    f.render_widget(Clear, area);

    let block = Block::default()
        .borders(Borders::ALL)
        .title(" Delete Playlist ")
        .border_style(Style::default().fg(Color::Red));

    let inner = block.inner(area);
    f.render_widget(block, area);

    let rows = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Length(1), Constraint::Length(1), Constraint::Length(1)])
        .split(inner);

    // Playlist name being deleted
    let pl_name = if app.playlist_cursor > 0 {
        app.playlists
            .get(app.playlist_cursor - 1)
            .map(|p| p.name.as_str())
            .unwrap_or("(unknown)")
    } else {
        "(unknown)"
    };

    let name_line = Line::from(vec![
        Span::raw(" Delete \""),
        Span::styled(pl_name, Style::default().fg(Color::Yellow)),
        Span::raw("\"?"),
    ]);
    f.render_widget(Paragraph::new(name_line), rows[0]);

    let warn_line = Line::from(Span::styled(
        " This cannot be undone.",
        Style::default().fg(Color::DarkGray),
    ));
    f.render_widget(Paragraph::new(warn_line), rows[1]);

    let hint = Line::from(vec![
        Span::styled(" Enter / y", Style::default().fg(Color::Red)),
        Span::raw(" delete  "),
        Span::styled("Esc / n", Style::default().fg(Color::Green)),
        Span::raw(" keep"),
    ]);
    f.render_widget(Paragraph::new(hint), rows[2]);
}
