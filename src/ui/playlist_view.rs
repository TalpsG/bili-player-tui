use ratatui::layout::Rect;
use ratatui::widgets::{Block, Borders, List, ListItem};
use ratatui::Frame;

use crate::app::{App, FocusColumn};

/// Draw the left column: playlist list. P1 only shows "Queue".
pub fn draw(f: &mut Frame, app: &App, area: Rect) {
    if area.width == 0 {
        return;
    }

    let items = vec![ListItem::new(format!(
        " Queue ({})",
        app.queue.len()
    ))];

    let border_style = if app.focus_column == FocusColumn::Playlist {
        app.ui.theme.focused_border
    } else {
        app.ui.theme.unfocused_border
    };

    let list = List::new(items).block(
        Block::default()
            .borders(Borders::ALL)
            .title("Playlists")
            .border_style(border_style),
    );

    f.render_widget(list, area);
}
