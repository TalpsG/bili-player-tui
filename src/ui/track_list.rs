use ratatui::layout::Rect;
use ratatui::style::{Modifier, Style};
use ratatui::widgets::{Block, Borders, List, ListItem, ListState};
use ratatui::Frame;

use crate::app::{App, FocusColumn};
use crate::ui::Ui;

/// Draw the middle column: queue track list.
pub fn draw(f: &mut Frame, app: &App, ui: &mut Ui, area: Rect) {
    if area.width == 0 {
        return;
    }

    let tracks = app.queue.tracks();
    let current_idx = app.queue.current_index();

    let items: Vec<ListItem> = tracks
        .iter()
        .enumerate()
        .map(|(i, track)| {
            let is_current = current_idx == Some(i);
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

    let list = List::new(items)
        .block(
            Block::default()
                .borders(Borders::ALL)
                .title("Tracks")
                .border_style(border_style),
        )
        .highlight_style(ui.theme.focused_item);

    let mut state = ListState::default();
    if !tracks.is_empty() {
        state.select(Some(ui.track_list_cursor));
    }
    f.render_stateful_widget(list, area, &mut state);
}
