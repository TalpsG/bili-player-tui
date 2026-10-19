use ratatui::Frame;
use ratatui::layout::{Alignment, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Paragraph, Wrap};

use crate::app::{App, FocusColumn};
use crate::cover::CoverManager;
use crate::queue::track::{AudioQuality, Track};

/// Draw the right column: now-playing detail (B-style: cover placeholder + centered metadata).
pub fn draw(f: &mut Frame, app: &App, cover_manager: &mut Option<CoverManager>, area: Rect) {
    draw_detail(f, app, cover_manager, area, false);
}

pub fn draw_fullscreen(
    f: &mut Frame,
    app: &App,
    cover_manager: &mut Option<CoverManager>,
    area: Rect,
) {
    draw_detail(f, app, cover_manager, area, true);
}

fn draw_detail(
    f: &mut Frame,
    app: &App,
    cover_manager: &mut Option<CoverManager>,
    area: Rect,
    fullscreen: bool,
) {
    if area.is_empty() {
        return;
    }

    let border_style = if app.focus_column == FocusColumn::Detail {
        app.ui.theme.focused_border
    } else {
        app.ui.theme.unfocused_border
    };

    // Right panel shows whatever row the middle-column cursor is on — a video
    // row or one of its parts (分P); falls back to now-playing.
    let cursor_track: Option<Track> = app.current_row().and_then(|row| app.row_track(row));
    let displayed_track = cursor_track.as_ref().or_else(|| app.queue.current_track());

    let block = Block::default()
        .borders(Borders::ALL)
        .title(" Details ")
        .border_style(border_style);

    let inner = block.inner(area);
    f.render_widget(block, area);

    if inner.is_empty() {
        return;
    }

    // Suppress cover rendering while the cursor is moving rapidly.
    // `cover_render_after` is set on every j/k; cover shows once the cursor
    // settles (debounce ~120 ms).  This eliminates Kitty/Sixel pixel-data
    // transmission on every keypress and makes cursor movement feel instant.
    let cover_suppressed = app
        .cover_render_after
        .map(|deadline| std::time::Instant::now() < deadline)
        .unwrap_or(false);

    match displayed_track {
        None => draw_empty(f, inner),
        Some(track) => {
            let mut displayed = track.clone();
            if displayed.page.is_some() && !app.has_visible_part(track) {
                displayed.part = None;
                displayed.page = None;
            }
            draw_track(
                f,
                inner,
                &displayed,
                cover_manager,
                cover_suppressed,
                fullscreen,
            );
        }
    }
}

// ── Empty state ──────────────────────────────────────────────────────────────

fn draw_empty(f: &mut Frame, area: Rect) {
    let lines = vec![
        Line::from(Span::styled(
            "♪",
            Style::default()
                .fg(Color::DarkGray)
                .add_modifier(Modifier::BOLD),
        )),
        Line::from(""),
        Line::from(Span::styled(
            "Nothing playing",
            Style::default().fg(Color::DarkGray),
        )),
    ];
    let content_h = lines.len() as u16;
    let offset = area.height.saturating_sub(content_h) / 2;
    let render_area = Rect {
        y: area.y + offset,
        height: area.height.saturating_sub(offset),
        ..area
    };
    f.render_widget(
        Paragraph::new(lines).alignment(Alignment::Center),
        render_area,
    );
}

// ── Track detail ─────────────────────────────────────────────────────────────

fn draw_track(
    f: &mut Frame,
    area: Rect,
    track: &Track,
    cover_manager: &mut Option<CoverManager>,
    cover_suppressed: bool,
    fullscreen: bool,
) {
    let (cover_area, text_area) = detail_areas(area, track.part.is_some(), fullscreen);
    if !cover_area.is_empty() {
        let rendered = !cover_suppressed
            && match (cover_manager, &track.cover_url) {
                (Some(mgr), Some(url)) => mgr.render_cover(url, cover_area, f),
                _ => false,
            };
        if !rendered {
            draw_cover_placeholder(f, cover_area);
        }
    }
    draw_track_info(f, text_area, track);
}

/// Reserve metadata before assigning remaining rows to artwork.
fn detail_areas(area: Rect, has_part: bool, fullscreen: bool) -> (Rect, Rect) {
    let metadata = if has_part { 5 } else { 4 };
    if fullscreen && area.width >= 90 && area.height >= 12 {
        let cover_width = (u32::from(area.width) * 3 / 5) as u16;
        let text = Rect::new(
            area.x.saturating_add(cover_width).saturating_add(2),
            area.y,
            area.width.saturating_sub(cover_width + 2),
            area.height,
        );
        return (Rect::new(area.x, area.y, cover_width, area.height), text);
    }
    let text_height = area.height.min(metadata + 2);
    let cover_height = area.height.saturating_sub(text_height);
    (
        Rect::new(area.x, area.y, area.width, cover_height),
        Rect::new(
            area.x,
            area.y.saturating_add(cover_height),
            area.width,
            text_height,
        ),
    )
}

/// ASCII box placeholder for the cover image area.
/// Will be replaced with a ratatui-image StatefulImage in the cover P2 phase.
fn draw_cover_placeholder(f: &mut Frame, area: Rect) {
    if area.height < 3 || area.width < 4 {
        return;
    }
    let inner_w = area.width.saturating_sub(2) as usize;
    let inner_h = area.height.saturating_sub(2) as usize;
    let dim = Style::default().fg(Color::DarkGray);

    let mut lines: Vec<Line> = Vec::with_capacity(area.height as usize);

    // Top border
    lines.push(Line::from(Span::styled(
        format!("┌{}┐", "─".repeat(inner_w)),
        dim,
    )));

    // Inner rows — ♪ centred vertically
    let note_row = inner_h / 2;
    for row in 0..inner_h {
        if row == note_row {
            let pad_l = inner_w.saturating_sub(1) / 2;
            let pad_r = inner_w.saturating_sub(pad_l + 1);
            lines.push(Line::from(vec![
                Span::styled("│", dim),
                Span::raw(" ".repeat(pad_l)),
                Span::styled("♪", dim),
                Span::raw(" ".repeat(pad_r)),
                Span::styled("│", dim),
            ]));
        } else {
            lines.push(Line::from(vec![
                Span::styled("│", dim),
                Span::raw(" ".repeat(inner_w)),
                Span::styled("│", dim),
            ]));
        }
    }

    // Bottom border
    lines.push(Line::from(Span::styled(
        format!("└{}┘", "─".repeat(inner_w)),
        dim,
    )));

    f.render_widget(Paragraph::new(lines), area);
}

/// Centred track metadata below the cover area.
fn draw_track_info(f: &mut Frame, area: Rect, track: &Track) {
    let mut lines: Vec<Line> = Vec::new();

    // Title — bold cyan, wraps if needed
    lines.push(Line::from(Span::styled(
        track.title.clone(),
        Style::default()
            .fg(Color::Cyan)
            .add_modifier(Modifier::BOLD),
    )));

    // Author — gray
    lines.push(Line::from(Span::styled(
        track.author.clone(),
        Style::default().fg(Color::Gray),
    )));

    // Part (分P) — only for page-level tracks
    if let Some(part) = &track.part {
        let label = match track.page {
            Some(page) => format!("P{page} · {part}"),
            None => part.clone(),
        };
        lines.push(Line::from(Span::styled(
            label,
            Style::default().fg(Color::Yellow),
        )));
    }

    // Duration (always present) — append quality only once source is resolved
    let dur = super::format_duration(Some(track.duration));
    let meta = match &track.source {
        Some(src) => format!("⏱  {}  ·  🎵 {}", dur, format_quality(&src.audio_quality)),
        None => format!("⏱  {}", dur),
    };
    lines.push(Line::from(Span::styled(
        meta,
        Style::default().fg(Color::Gray),
    )));

    // BV number — dimmed
    lines.push(Line::from(Span::styled(
        format!("🔖 {}", track.bvid),
        Style::default().fg(Color::DarkGray),
    )));

    // Render each field separately so a long wrapped title cannot consume
    // the author, part, duration, or BV rows.
    if area.is_empty() {
        return;
    }
    let fields = lines.len() as u16;
    let title_height = area
        .height
        .saturating_sub(fields.saturating_sub(1))
        .clamp(1, 3)
        .min(area.height);
    let used = title_height
        .saturating_add(fields.saturating_sub(1))
        .min(area.height);
    let mut y = area.y.saturating_add(area.height.saturating_sub(used) / 2);
    for (index, line) in lines.into_iter().enumerate() {
        let height = if index == 0 { title_height } else { 1 };
        if y >= area.bottom() {
            break;
        }
        let row = Rect::new(area.x, y, area.width, height.min(area.bottom() - y));
        f.render_widget(
            Paragraph::new(line)
                .alignment(Alignment::Center)
                .wrap(Wrap { trim: true }),
            row,
        );
        y = y.saturating_add(height);
    }
}

// ── Helpers ──────────────────────────────────────────────────────────────────

fn format_quality(q: &AudioQuality) -> &'static str {
    match q {
        AudioQuality::Flac => "FLAC",
        AudioQuality::DolbyAtmos => "Dolby",
        AudioQuality::Dash { bitrate: 30280 } => "320k",
        AudioQuality::Dash { bitrate: 30232 } => "192k",
        AudioQuality::Dash { bitrate: 30216 } => "64k",
        AudioQuality::Dash { .. } => "AAC",
        AudioQuality::LegacyMp4 => "MP4",
    }
}

#[cfg(test)]
mod responsive_tests {
    use super::*;

    #[test]
    fn details_reserve_metadata_and_switch_orientation() {
        for has_part in [false, true] {
            let (cover, text) = detail_areas(Rect::new(2, 3, 40, 20), has_part, false);
            assert_eq!(cover.bottom(), text.y);
            assert!(text.height >= if has_part { 5 } else { 4 });
            let (cover, text) = detail_areas(Rect::new(2, 3, 100, 20), has_part, true);
            assert!(cover.right() < text.x);
            let (cover, text) = detail_areas(Rect::new(0, 0, 20, 3), has_part, true);
            assert_eq!(cover.height, 0);
            assert_eq!(text.height, 3);
        }
    }
}
