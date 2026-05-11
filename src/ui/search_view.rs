use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::Span;
use ratatui::widgets::{Block, Borders, Clear, List, ListItem, ListState, Paragraph};
use ratatui::Frame;

use crate::app::{App, InputMode};

/// Draw search overlay: centered popup with input box + result list.
pub fn draw(f: &mut Frame, app: &App, screen: Rect) {
    let width = 60u16.min(screen.width.saturating_sub(4));
    let height = 20u16.min(screen.height.saturating_sub(4));
    let area = super::popup::popup_area(screen, super::popup::Anchor::Center, width, height);

    // Clear the background
    f.render_widget(Clear, area);

    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Length(3), Constraint::Min(0)])
        .split(area);

    let _is_input_mode = app.input_mode == InputMode::SearchInput;
    let mode_str = match app.input_mode {
        InputMode::SearchInput => " -- INSERT -- ",
        InputMode::SearchNormal => " -- NORMAL -- ",
        _ => "",
    };

    // Draw input box
    let input_text = format!("/{}", app.search_query);
    let input_title = format!("Search{}", mode_str);
    
    let input_border_style = if app.search_focus_input {
        app.ui.theme.focused_border
    } else {
        app.ui.theme.unfocused_border
    };

    let input = Paragraph::new(input_text)
        .block(
            Block::default()
                .borders(Borders::ALL)
                .title(input_title)
                .border_style(input_border_style),
        )
        .style(app.ui.theme.search_input);
    f.render_widget(input, chunks[0]);

    // Draw results list
    let results_border_style = if !app.search_focus_input {
        app.ui.theme.focused_border
    } else {
        app.ui.theme.unfocused_border
    };

    let items: Vec<ListItem> = app
        .search_results
        .iter()
        .enumerate()
        .map(|(i, track)| {
            let is_selected = i == app.search_cursor;
            let style = if is_selected && !app.search_focus_input {
                app.ui.theme.focused_item.add_modifier(Modifier::BOLD)
            } else if is_selected {
                Style::default().fg(Color::Cyan).add_modifier(Modifier::UNDERLINED)
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
            .border_style(results_border_style),
    );

    let mut state = ListState::default();
    if !app.search_results.is_empty() {
        state.select(Some(app.search_cursor));
    }
    f.render_stateful_widget(list, chunks[1], &mut state);

    // Set cursor position
    if app.search_focus_input {
        // Use unicode-width for CJK characters
        let query_prefix: String = app.search_query.chars().take(app.search_query_cursor).collect();
        let query_width: usize = query_prefix.chars()
            .map(|c| unicode_width::UnicodeWidthChar::width(c).unwrap_or(0))
            .sum();
        
        let cursor_x = chunks[0].x + 1 + 1 + query_width as u16;
        let cursor_y = chunks[0].y + 1;
        
        // In Normal mode on input, we might want a different cursor look, 
        // but terminal cursor is usually a block or line.
        f.set_cursor_position((cursor_x, cursor_y));
    }
}
