use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::style::{Modifier, Style};
use ratatui::widgets::{Block, Borders, List, ListItem, ListState};

use crate::app::{App, FocusColumn, TrackRow};
use crate::ui::Ui;

/// Extra indent for part rows, on top of the four columns a video row spends on
/// its fold marker and play arrow.  Parts then read as children of the video
/// above them instead of blending into the list.
const PART_ROW_INDENT: &str = "    ";

/// Draw the middle column: either the playback queue or a playlist's tracks.
///
/// Multi-part videos (分P) render as a foldable tree: the video row carries a
/// `▾`/`▸` marker once its part list is known, and expanded videos own one
/// indented row per part.
pub fn draw(f: &mut Frame, app: &App, ui: &mut Ui, area: Rect) {
    if area.width == 0 {
        return;
    }

    let is_queue_view = app.playlist_cursor == 0;
    let current_idx = if is_queue_view {
        app.queue.current_index()
    } else {
        None
    };
    let rows = app.visible_rows();

    let items: Vec<ListItem> = rows
        .iter()
        .filter_map(|&row| {
            let track = app.row_track(row)?;
            let track_idx = row.track_index();

            // A part row is never the current queue entry -- its parent video
            // is -- so only real queue entries can carry the playing highlight.
            let is_page_row = matches!(row, TrackRow::Page(_, _));
            let is_current = is_queue_view && current_idx == Some(track_idx) && !is_page_row;
            let is_part = is_page_row || track.page.is_some();

            let is_header = matches!(row, TrackRow::PartGroupHeader(_));
            let style = if is_current {
                ui.theme.selected_item.add_modifier(Modifier::BOLD)
            } else if is_header {
                // Group titles sit between the dimmed parts and the plain video
                // rows, so they read as headings rather than as queue entries.
                Style::default()
                    .fg(ui.theme.text_primary)
                    .add_modifier(Modifier::BOLD)
            } else if is_part {
                // Parts are dimmer so the video rows stay scannable.
                Style::default().fg(ui.theme.text_secondary)
            } else {
                Style::default()
            };

            let title = match row {
                TrackRow::PartGroupHeader(_) => {
                    format!("    {} - {}", track.display_title(), track.author)
                }
                TrackRow::Video(_) => {
                    let marker = fold_marker(app, &track);
                    // `▶` marks the currently playing row; `▾`/`▸` is the fold
                    // state, so the two never collide.
                    let playing = if is_current { "▶ " } else { "  " };
                    match track.page {
                        // A queued part: name it after the part, otherwise
                        // every part of one video looks identical in the queue.
                        Some(page) => format!(
                            "{marker}{playing}{PART_ROW_INDENT}P{page} · {}",
                            track.display_title()
                        ),
                        None => format!("{marker}{playing}{} - {}", track.title, track.author),
                    }
                }
                TrackRow::Page(_, _) => {
                    let page = track.page.unwrap_or(0);
                    format!("    {PART_ROW_INDENT}P{page} · {}", track.display_title())
                }
            };

            Some(ListItem::new(ratatui::text::Span::styled(title, style)))
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
    if !rows.is_empty() && ui.track_list_cursor < rows.len() {
        state.select(Some(ui.track_list_cursor));
    }
    f.render_stateful_widget(list, area, &mut state);
}

/// Leading marker for a video row: fold state, or a loading hint.
fn fold_marker(app: &App, track: &crate::queue::track::Track) -> &'static str {
    if track.page.is_some() {
        // A part-level track has no children.
        return "  ";
    }
    match app.pages_cache.get(&track.bvid).map(|pages| pages.len()) {
        Some(n) if n > 1 => {
            if app.expanded.contains(&track.bvid) {
                "▾ "
            } else {
                "▸ "
            }
        }
        Some(_) => "  ",
        None if app.is_pages_loading(&track.bvid) => "… ",
        None => "  ",
    }
}
