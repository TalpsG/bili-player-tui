use ratatui::style::{Color, Style};

pub struct Theme {
    pub header_bg: Color,
    pub header_fg: Color,
    pub status_bg: Color,
    pub status_fg: Color,
    pub focused_border: Style,
    pub unfocused_border: Style,
    pub selected_item: Style,
    pub focused_item: Style,
    pub gauge_filled: Style,
    pub gauge_unfilled: Style,
    pub popup_border: Style,
    pub search_input: Style,
    pub text_primary: Color,
    pub text_secondary: Color,
}

impl Default for Theme {
    fn default() -> Self {
        Self {
            header_bg: Color::DarkGray,
            header_fg: Color::White,
            status_bg: Color::DarkGray,
            status_fg: Color::White,
            focused_border: Style::default().fg(Color::Cyan),
            unfocused_border: Style::default().fg(Color::DarkGray),
            selected_item: Style::default().fg(Color::Green),
            focused_item: Style::default().bg(Color::Blue).fg(Color::White),
            gauge_filled: Style::default().fg(Color::Cyan).bg(Color::DarkGray),
            gauge_unfilled: Style::default().fg(Color::DarkGray).bg(Color::Black),
            popup_border: Style::default().fg(Color::Cyan),
            search_input: Style::default().fg(Color::Yellow),
            text_primary: Color::White,
            text_secondary: Color::Gray,
        }
    }
}
