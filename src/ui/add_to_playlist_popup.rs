use ratatui::layout::Rect;
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Clear, List, ListItem, ListState};
use ratatui::Frame;

use crate::app::App;

/// Draw the "Add to Playlist" chooser popup.
///
/// Shows all user playlists (not Queue). j/k to move, Enter to confirm, Esc/q to cancel.
pub fn draw(f: &mut Frame, app: &App, screen: Rect) {
    // Fit height to number of playlists (min 5, max 15), plus borders + header + footer = 4
    let list_height = (app.playlists.len() as u16 + 2).clamp(3, 13);
    let height = list_height + 4; // border top + title + footer + border bottom
    let width = 50u16.min(screen.width);

    let area = super::popup::popup_area(
        screen,
        super::popup::Anchor::Center,
        width,
        height,
    );

    f.render_widget(Clear, area);

    let block = Block::default()
        .borders(Borders::ALL)
        .title(" Add to Playlist ")
        .border_style(app.ui.theme.popup_border);

    let inner = block.inner(area);
    f.render_widget(block, area);

    if app.playlists.is_empty() {
        // No playlists yet — show a message
        let msg = Line::from(Span::styled(
            " No playlists. Press 'c' in the playlist column to create one.",
            Style::default().fg(Color::DarkGray),
        ));
        f.render_widget(ratatui::widgets::Paragraph::new(msg), inner);
        return;
    }

    // Build list: show track name being added at top, then playlist items
    let track_title = app
        .add_to_playlist_track
        .as_ref()
        .map(|t| t.title.as_str())
        .unwrap_or("(no track)");

    // Reserve bottom row for hint; use remaining rows for list
    let list_area = ratatui::layout::Rect {
        height: inner.height.saturating_sub(2),
        ..inner
    };
    let hint_area = ratatui::layout::Rect {
        y: inner.y + inner.height.saturating_sub(2),
        height: 1,
        ..inner
    };
    let label_area = ratatui::layout::Rect {
        y: inner.y,
        height: 1,
        ..inner
    };
    let actual_list_area = ratatui::layout::Rect {
        y: inner.y + 1,
        height: inner.height.saturating_sub(2),
        ..inner
    };

    // "Adding: <title>" label
    let label = Line::from(vec![
        Span::styled(" Adding: ", Style::default().fg(Color::DarkGray)),
        Span::styled(track_title, Style::default().fg(Color::Cyan).add_modifier(Modifier::ITALIC)),
    ]);
    let _ = list_area; // suppress unused warning
    f.render_widget(ratatui::widgets::Paragraph::new(label), label_area);

    // Playlist list
    let items: Vec<ListItem> = app
        .playlists
        .iter()
        .enumerate()
        .map(|(i, pl)| {
            let style = if i == app.add_to_playlist_cursor {
                app.ui.theme.selected_item.add_modifier(Modifier::BOLD)
            } else {
                Style::default()
            };
            let label = format!(" {} ({})", pl.name, pl.tracks.len());
            ListItem::new(Span::styled(label, style))
        })
        .collect();

    let list = List::new(items).highlight_style(app.ui.theme.focused_item);

    let mut state = ListState::default();
    state.select(Some(app.add_to_playlist_cursor));
    f.render_stateful_widget(list, actual_list_area, &mut state);

    // Hint footer
    let hint = Line::from(vec![
        Span::styled(" Enter", Style::default().fg(Color::Green)),
        Span::raw(" add  "),
        Span::styled("Esc / q", Style::default().fg(Color::Red)),
        Span::raw(" cancel"),
    ]);
    f.render_widget(ratatui::widgets::Paragraph::new(hint), hint_area);
}
