use ratatui::layout::Rect;
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Paragraph, Wrap};
use ratatui::Frame;

use crate::app::{App, FocusColumn};

/// Draw the right column: now-playing detail.
pub fn draw(f: &mut Frame, app: &App, area: Rect) {
    if area.width == 0 {
        return;
    }

    let border_style = if app.focus_column == FocusColumn::Detail {
        app.ui.theme.focused_border
    } else {
        app.ui.theme.unfocused_border
    };

    let track = match app.queue.current_track() {
        Some(t) => t,
        None => {
            let empty = Paragraph::new("No track playing").block(
                Block::default()
                    .borders(Borders::ALL)
                    .title("Now Playing")
                    .border_style(border_style),
            );
            f.render_widget(empty, area);
            return;
        }
    };

    let duration_str = super::format_duration(Some(track.duration));
    let quality_str = track
        .source
        .as_ref()
        .map(|s| format!("{:?}", s.audio_quality))
        .unwrap_or_else(|| "—".to_string());

    let lines = vec![
        Line::from(Span::styled(
            &track.title,
            Style::default()
                .fg(Color::Cyan)
                .add_modifier(Modifier::BOLD),
        )),
        Line::from(""),
        Line::from(vec![
            Span::styled("Author: ", Style::default().fg(Color::Gray)),
            Span::raw(&track.author),
        ]),
        Line::from(vec![
            Span::styled("Duration: ", Style::default().fg(Color::Gray)),
            Span::raw(&duration_str),
        ]),
        Line::from(vec![
            Span::styled("BV: ", Style::default().fg(Color::Gray)),
            Span::raw(&track.bvid),
        ]),
        Line::from(vec![
            Span::styled("Quality: ", Style::default().fg(Color::Gray)),
            Span::raw(&quality_str),
        ]),
    ];

    let paragraph = Paragraph::new(lines)
        .block(
            Block::default()
                .borders(Borders::ALL)
                .title("Now Playing")
                .border_style(border_style),
        )
        .wrap(Wrap { trim: true });
    f.render_widget(paragraph, area);
}
