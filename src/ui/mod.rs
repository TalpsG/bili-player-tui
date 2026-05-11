pub mod add_to_playlist_popup;
pub mod confirm_popup;
pub mod controls;
pub mod help_view;
pub mod input_popup;
pub mod layout;
pub mod now_playing;
pub mod playlist_view;
pub mod popup;
pub mod search_view;
pub mod theme;
pub mod track_list;
pub mod volume_slider;

#[cfg(test)]
mod tests;

use ratatui::layout::Rect;
use ratatui::style::Style;
use ratatui::widgets::{Gauge, Paragraph};
use ratatui::Frame;

use crate::app::App;

/// UI state and rendering.
pub struct Ui {
    pub theme: theme::Theme,
    pub track_list_cursor: usize,
    pub track_list_scroll: usize,
    pub search_cursor: usize,
    pub playlist_list_state: ratatui::widgets::ListState,
}

impl Ui {
    pub fn new() -> Self {
        Self {
            theme: theme::Theme::default(),
            track_list_cursor: 0,
            track_list_scroll: 0,
            search_cursor: 0,
            playlist_list_state: ratatui::widgets::ListState::default(),
        }
    }
}

impl Default for Ui {
    fn default() -> Self {
        Self::new()
    }
}

/// Draw header bar.
pub fn draw_header(f: &mut Frame, app: &App, area: Rect) {
    use unicode_width::UnicodeWidthStr;
    let left = " bili-player-cli";
    let right = if app.logged_in { "已登录" } else { "未登录" };
    
    let left_width = left.width();
    let right_width = right.width();
    let total_width = area.width as usize;
    
    let space_count = total_width.saturating_sub(left_width + right_width);
    let padded = format!("{}{}{}", left, " ".repeat(space_count), right);

    let header = Paragraph::new(padded)
        .style(Style::default().fg(app.ui.theme.header_fg).bg(app.ui.theme.header_bg));
    f.render_widget(header, area);
}

/// Draw status bar.
pub fn draw_status_bar(f: &mut Frame, app: &App, area: Rect) {
    use ratatui::layout::{Constraint, Direction, Layout};

    let chunks = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(60), Constraint::Percentage(40)])
        .split(area);

    // Progress gauge
    let ratio = match (app.position, app.duration) {
        (Some(p), Some(d)) if d.as_secs() > 0 => p.as_secs_f64() / d.as_secs_f64(),
        _ => 0.0,
    };
    let pos_str = format_duration(app.position);
    let dur_str = format_duration(app.duration);
    let label = format!("{pos_str}/{dur_str}");

    let gauge = Gauge::default()
        .gauge_style(app.ui.theme.gauge_filled)
        .ratio(ratio.min(1.0))
        .label(label);
    f.render_widget(gauge, chunks[0]);

    // Volume + status
    let vol_icon = if app.muted { "🔇" } else { "🔊" };
    let status_text = match &app.status_message {
        Some(msg) => msg.clone(),
        None => {
            let mode_icon = app.queue.play_mode.icon();
            if mode_icon.is_empty() {
                format!("{vol_icon}{}", app.volume)
            } else {
                format!("{mode_icon} {vol_icon}{}", app.volume)
            }
        }
    };
    // Right-align status_text, accounting for wide characters (emoji = 2 cols each).
    let available = chunks[1].width as usize;
    let text_display_width = unicode_width::UnicodeWidthStr::width(status_text.as_str());
    let padding = available.saturating_sub(text_display_width);
    let padded = format!("{}{}", " ".repeat(padding), status_text);
    let info = Paragraph::new(padded)
        .style(Style::default().fg(app.ui.theme.status_fg).bg(app.ui.theme.status_bg));
    f.render_widget(info, chunks[1]);
}

/// Format a Duration as M:SS or H:MM:SS.
pub fn format_duration(d: Option<std::time::Duration>) -> String {
    match d {
        Some(d) => {
            let total_secs = d.as_secs();
            let hours = total_secs / 3600;
            let mins = (total_secs % 3600) / 60;
            let secs = total_secs % 60;
            if hours > 0 {
                format!("{hours}:{mins:02}:{secs:02}")
            } else {
                format!("{mins}:{secs:02}")
            }
        }
        None => "--:--".to_string(),
    }
}
