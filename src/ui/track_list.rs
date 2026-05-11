use ratatui::layout::Rect;
use ratatui::style::{Modifier, Style};
use ratatui::widgets::{Block, Borders, List, ListItem, ListState};
use ratatui::Frame;

use crate::app::{App, FocusColumn};
use crate::ui::Ui;

/// Draw the middle column: either the playback queue or a playlist's tracks.
pub fn draw(f: &mut Frame, app: &App, ui: &mut Ui, area: Rect) {
    if area.width == 0 {
        return;
    }

    let is_queue_view = app.playlist_cursor == 0;
    let tracks = app.active_track_list();
    let current_idx = if is_queue_view { app.queue.current_index() } else { None };

    let items: Vec<ListItem> = tracks
        .iter()
        .enumerate()
        .map(|(i, track)| {
            let is_current = is_queue_view && current_idx == Some(i);
            let prefix = if is_current { " ▸ " } else { "   " };
            let style = if is_current {
                ui.theme.selected_item.add_modifier(Modifier::BOLD)
            } else {
                Style::default()
            };
            let title = format!("{}{} - {}", prefix, track.title, track.author);
            ListItem::new(ratatui::text::Span::styled(title, style))
        })
        .collect();

    let border_style = if app.focus_column == FocusColumn::TrackList {
        app.ui.theme.focused_border
    } else {
        app.ui.theme.unfocused_border
    };

    let title = if is_queue_view {
        "Queue".to_string()
    } else {
        let pl_idx = app.playlist_cursor - 1;
        app.playlists
            .get(pl_idx)
            .map(|p| p.name.clone())
            .unwrap_or_else(|| "Tracks".to_string())
    };

    let list = List::new(items)
        .block(
            Block::default()
                .borders(Borders::ALL)
                .title(title)
                .border_style(border_style),
        )
        .highlight_style(ui.theme.focused_item);

    let mut state = ListState::default();
    if !tracks.is_empty() {
        state.select(Some(ui.track_list_cursor));
    }
    f.render_stateful_widget(list, area, &mut state);
}
