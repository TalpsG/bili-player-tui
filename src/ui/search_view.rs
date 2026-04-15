use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::style::{Color, Style};
use ratatui::text::Span;
use ratatui::widgets::{Block, Borders, List, ListItem, ListState, Paragraph};
use ratatui::Frame;

use crate::app::App;
use crate::ui::Ui;

/// Draw search overlay: centered popup with input box + result list.
pub fn draw(f: &mut Frame, app: &App, ui: &mut Ui, screen: Rect) {
    let width = 60u16.min(screen.width.saturating_sub(4));
    let height = 20u16.min(screen.height.saturating_sub(4));
    let area = super::popup::popup_area(screen, super::popup::Anchor::Center, width, height);

    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Length(3), Constraint::Min(0)])
        .split(area);

    // Draw input box
    let input_text = format!("/{}", app.search_query);
    let input = Paragraph::new(input_text)
        .block(
            Block::default()
                .borders(Borders::ALL)
                .title("Search")
                .style(app.ui.theme.popup_border),
        )
        .style(app.ui.theme.search_input);
    f.render_widget(input, chunks[0]);

    // Set cursor position in input
    let cursor_x = chunks[0].x + 1 + 1 + app.search_query.len() as u16;
    let cursor_y = chunks[0].y + 1;
    f.set_cursor_position((cursor_x, cursor_y));

    // Draw results list
    let items: Vec<ListItem> = app
        .search_results
        .iter()
        .enumerate()
        .map(|(i, track)| {
            let style = if i == ui.search_cursor {
                Style::default().bg(Color::Blue).fg(Color::White)
            } else {
                Style::default()
            };
            let title = format!("{} - {}", track.title, track.author);
            ListItem::new(Span::styled(title, style))
        })
        .collect();

    let list = List::new(items).block(
        Block::default()
            .borders(Borders::ALL)
            .title(if app.searching {
                "Searching..."
            } else {
                "Results"
            })
            .style(app.ui.theme.popup_border),
    );

    let mut state = ListState::default();
    if !app.search_results.is_empty() {
        state.select(Some(ui.search_cursor));
    }
    f.render_stateful_widget(list, chunks[1], &mut state);
}
