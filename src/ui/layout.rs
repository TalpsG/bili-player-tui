use ratatui::layout::{Constraint, Direction, Layout, Rect};

/// Column visibility based on terminal width.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ColumnVisibility {
    pub playlist: bool,
    pub track_list: bool,
    pub detail: bool,
}

impl ColumnVisibility {
    pub fn from_width(width: u16) -> Self {
        if width >= 80 {
            Self { playlist: true, track_list: true, detail: true }
        } else if width >= 50 {
            Self { playlist: true, track_list: true, detail: false }
        } else {
            Self { playlist: false, track_list: true, detail: false }
        }
    }
}

/// Compute the main layout: Header, Body, Status.
pub fn main_layout(area: Rect) -> (Rect, Rect, Rect) {
    let cols = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(1),  // Header
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
    let constraints = match (visibility.playlist, visibility.track_list, visibility.detail) {
        (true, true, true) => vec![
            Constraint::Ratio(1, 6),
            Constraint::Ratio(3, 6),
            Constraint::Ratio(2, 6),
        ],
        (true, true, false) => vec![
            Constraint::Ratio(1, 4),
            Constraint::Ratio(3, 4),
        ],
        (false, true, false) => vec![
            Constraint::Percentage(100),
        ],
        _ => vec![Constraint::Percentage(100)],
    };

    let columns = Layout::default()
        .direction(Direction::Horizontal)
        .constraints(constraints)
        .split(body);

    let playlist = if visibility.playlist { columns[0] } else { Rect::default() };
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
