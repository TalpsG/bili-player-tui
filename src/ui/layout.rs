use ratatui::layout::{Constraint, Direction, Layout, Rect};

/// Column visibility based on terminal width.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ColumnVisibility {
    pub playlist: bool,
    pub track_list: bool,
    pub detail: bool,
}

impl ColumnVisibility {
    pub fn from_size(width: u16, height: u16) -> Self {
        Self {
            playlist: width >= 72,
            track_list: true,
            detail: width >= 110 && height >= 18,
        }
    }

    pub fn from_width(width: u16) -> Self {
        Self::from_size(width, 24)
    }
}

/// Compute the main layout: Header, Body, Status.
pub fn main_layout(area: Rect) -> (Rect, Rect, Rect) {
    let cols = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(1), // Header
            Constraint::Min(0),    // Body
            Constraint::Length(1), // Status
        ])
        .split(area);

    (cols[0], cols[1], cols[2])
}

/// Compute three-column layout for the body area.
/// Returns (playlist, track_list, detail) rects.
/// Hidden columns get Rect::default() (zero area).
pub fn body_columns(body: Rect, visibility: ColumnVisibility) -> (Rect, Rect, Rect) {
    // Sidebars grow slowly within bounds; the track list owns all remaining space.
    let playlist_width = (18 + body.width.saturating_sub(110) / 8).min(26);
    let detail_width = (28 + body.width.saturating_sub(110) / 6).min(40);
    let constraints = match (
        visibility.playlist,
        visibility.track_list,
        visibility.detail,
    ) {
        (true, true, true) => vec![
            Constraint::Length(playlist_width),
            Constraint::Min(56),
            Constraint::Length(detail_width),
        ],
        (true, true, false) => vec![Constraint::Length(playlist_width), Constraint::Min(0)],
        _ => vec![Constraint::Min(0)],
    };

    let columns = Layout::default()
        .direction(Direction::Horizontal)
        .constraints(constraints)
        .split(body);

    let playlist = if visibility.playlist {
        columns[0]
    } else {
        Rect::default()
    };
    let track_list = if visibility.track_list {
        let idx = if visibility.playlist { 1 } else { 0 };
        columns[idx]
    } else {
        Rect::default()
    };
    let detail = if visibility.detail {
        columns[columns.len() - 1]
    } else {
        Rect::default()
    };

    (playlist, track_list, detail)
}
