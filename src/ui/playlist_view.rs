use ratatui::layout::Rect;
use ratatui::style::{Modifier, Style};
use ratatui::widgets::{Block, Borders, List, ListItem};
use ratatui::Frame;

use crate::app::{App, FocusColumn};

/// Draw the left column: "Queue" + user playlists.
pub fn draw(f: &mut Frame, app: &App, area: Rect) {
    if area.width == 0 {
        return;
    }

    let mut items: Vec<ListItem> = Vec::with_capacity(1 + app.playlists.len());

    // Item 0 – Queue (virtual)
    let queue_label = format!(" Queue ({})", app.queue.len());
    let queue_style = if app.playlist_cursor == 0 {
        app.ui.theme.selected_item.add_modifier(Modifier::BOLD)
    } else {
        Style::default()
    };
    items.push(ListItem::new(ratatui::text::Span::styled(queue_label, queue_style)));

    // Items 1..N – user playlists
    for (i, pl) in app.playlists.iter().enumerate() {
        let label = format!(" {} ({})", pl.name, pl.tracks.len());
        let style = if app.playlist_cursor == i + 1 {
            app.ui.theme.selected_item.add_modifier(Modifier::BOLD)
        } else {
            Style::default()
        };
        items.push(ListItem::new(ratatui::text::Span::styled(label, style)));
    }

    let border_style = if app.focus_column == FocusColumn::Playlist {
        app.ui.theme.focused_border
    } else {
        app.ui.theme.unfocused_border
    };

    let list = List::new(items)
        .block(
            Block::default()
                .borders(Borders::ALL)
                .title("Playlists")
                .border_style(border_style),
        )
        .highlight_style(app.ui.theme.focused_item);

    // Copy the ListState to avoid borrow conflict (draw() receives &App not &mut App)
    let mut state = app.ui.playlist_list_state;
    f.render_stateful_widget(list, area, &mut state);
}
