use ratatui::Frame;
use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::Span;
use ratatui::widgets::{Block, Borders, Clear, List, ListItem, ListState, Paragraph};

use crate::app::{App, InputMode, PopupLayer};

/// Draw search overlay: centered popup with input box + result list.
pub fn draw(f: &mut Frame, app: &App, ui: &mut super::Ui, screen: Rect) {
    let width = ((u32::from(screen.width) * 3 / 4) as u16).clamp(60, 100);
    let height = ((u32::from(screen.height) * 3 / 4) as u16).min(30);
    let area = super::popup::popup_area(screen, super::popup::Anchor::Center, width, height);

    // Clear the background
    f.render_widget(Clear, area);

    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Length(3), Constraint::Min(0)])
        .split(area);

    let mode_str = match app.input_mode {
        InputMode::SearchInput => " -- INSERT -- ",
        InputMode::SearchNormal => " -- NORMAL -- ",
        InputMode::SearchResults => " -- RESULTS -- ",
        _ => "",
    };

    // Draw input box
    let input_text = format!("/{}", app.search_query);
    let input_block = Block::default().borders(Borders::ALL);
    let input_inner = input_block.inner(chunks[0]);
    let (input_text, cursor_column) = super::popup::input_window(
        &input_text,
        app.search_query_cursor.saturating_add(1),
        input_inner.width,
    );
    let hint = match app.input_mode {
        InputMode::SearchInput => "Enter search · Esc normal",
        InputMode::SearchNormal => "i/a edit · j results · Esc close",
        _ => "i edit · Esc query",
    };
    let input_title = format!("Search{}", mode_str);

    let input_border_style = if app.input_mode.search_focus_input() {
        app.ui.theme.focused_border
    } else {
        app.ui.theme.unfocused_border
    };

    let input = Paragraph::new(input_text)
        .block(
            Block::default()
                .borders(Borders::ALL)
                .title(input_title)
                .title_bottom(hint)
                .border_style(input_border_style),
        )
        .style(app.ui.theme.search_input);
    f.render_widget(input, chunks[0]);

    // Draw results list
    let results_border_style = if !app.input_mode.search_focus_input() {
        app.ui.theme.focused_border
    } else {
        app.ui.theme.unfocused_border
    };

    let row_width = usize::from(chunks[1].width.saturating_sub(2));
    let active = app.input_mode == InputMode::SearchResults
        && !app.popup_stack.iter().any(|layer| {
            matches!(
                layer,
                PopupLayer::Help
                    | PopupLayer::PlaylistCreate
                    | PopupLayer::PlaylistDeleteConfirm
                    | PopupLayer::AddToPlaylist
            )
        });
    if !active {
        ui.search_marquee.reset();
    }
    let items: Vec<ListItem> = app
        .search_results
        .iter()
        .enumerate()
        .map(|(i, track)| {
            let is_selected = i == app.search_cursor;
            let style = if is_selected && !app.input_mode.search_focus_input() {
                app.ui.theme.focused_item.add_modifier(Modifier::BOLD)
            } else if is_selected {
                Style::default()
                    .fg(Color::Cyan)
                    .add_modifier(Modifier::UNDERLINED)
            } else {
                Style::default()
            };
            let title = format!("{} - {}", track.title, track.author);
            let title = if active && is_selected {
                // Include the entire screen geometry so every resize starts at the head.
                let identity = format!(
                    "{screen:?}:{}:{}:{}:{title}",
                    app.search_query, app.search_cursor, track.bvid
                );
                let offset = ui.search_marquee.offset(
                    &identity,
                    &title,
                    row_width,
                    std::time::Instant::now(),
                );
                super::marquee::window(&title, offset, row_width)
            } else {
                title
            };
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

    if app.input_mode.search_focus_input() && input_inner.width > 0 && input_inner.height > 0 {
        f.set_cursor_position((input_inner.x.saturating_add(cursor_column), input_inner.y));
    }
}
