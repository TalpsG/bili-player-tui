use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::widgets::{Block, Borders, Clear, Gauge};

use crate::app::App;

/// Draw volume slider popup above status bar, centered.
pub fn draw(f: &mut Frame, app: &App, screen: Rect) {
    let width = 30u16;
    let height = 3;
    let area =
        super::popup::popup_area(screen, super::popup::Anchor::AboveStatusBar, width, height);

    let label = if app.muted {
        format!("MUTED ({}%)", app.volume)
    } else {
        format!("Volume: {}%", app.volume)
    };

    let gauge = Gauge::default()
        .block(
            Block::default()
                .borders(Borders::ALL)
                .style(app.ui.theme.popup_border),
        )
        .gauge_style(app.ui.theme.gauge_filled)
        .ratio(f64::from(app.volume.min(100)) / 100.0)
        .label(label);

    f.render_widget(Clear, area);
    f.render_widget(gauge, area);
}
