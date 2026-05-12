use ratatui::layout::{Alignment, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Paragraph, Wrap};
use ratatui::Frame;

use crate::app::{App, FocusColumn};
use crate::cover::CoverManager;
use crate::queue::track::{AudioQuality, Track};

/// Draw the right column: now-playing detail (B-style: cover placeholder + centered metadata).
pub fn draw(f: &mut Frame, app: &App, cover_manager: &mut Option<CoverManager>, area: Rect) {
    if area.width == 0 {
        return;
    }

    let border_style = if app.focus_column == FocusColumn::Detail {
        app.ui.theme.focused_border
    } else {
        app.ui.theme.unfocused_border
    };

    // Right panel shows the track under the cursor; falls back to now-playing
    // when the cursor points outside the list or no track is selected.
    let cursor_track: Option<&Track> = {
        let tracks = app.active_track_list();
        let idx = app.ui.track_list_cursor;
        tracks.get(idx)
    };
    let displayed_track = cursor_track.or_else(|| app.queue.current_track());

    let block = Block::default()
        .borders(Borders::ALL)
        .title(" Details ")
        .border_style(border_style);

    let inner = block.inner(area);
    f.render_widget(block, area);

    if inner.height < 4 || inner.width < 4 {
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
        Some(track) => draw_track(f, inner, track, cover_manager, cover_suppressed),
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

fn draw_track(f: &mut Frame, area: Rect, track: &Track, cover_manager: &mut Option<CoverManager>, cover_suppressed: bool) {
    // Text lines: title + author + meta + bvid = 4 (no blank line).
    const TEXT_LINES: u16 = 4;

    // Cover height: fill as much space as possible given two constraints:
    //   • must leave TEXT_LINES rows below for metadata
    //   • terminal chars are ~2:1 (w:h), so a square cover needs width = height×2;
    //     if width is narrow, cap cover_h at width/2 to avoid a stretched rectangle.
    let height_avail = area.height.saturating_sub(TEXT_LINES);
    let width_avail = area.width / 2;
    let cover_h = height_avail.min(width_avail).max(4);

    // Total block = cover + text (no gap — cover sits directly above text).
    let block_h = cover_h.saturating_add(TEXT_LINES);

    // Vertically centre the whole block.
    let v_offset = area.height.saturating_sub(block_h) / 2;
    let block_top = area.y + v_offset;

    // Cover width = height×2 for square appearance; centre horizontally.
    let cover_w = cover_h.saturating_mul(2).min(area.width);
    let x_offset = (area.width.saturating_sub(cover_w)) / 2;

    let cover_area = Rect {
        x: area.x + x_offset,
        y: block_top,
        width: cover_w,
        height: cover_h,
    };

    // Try to render the actual cover image; fall back to ASCII placeholder.
    // Skip the image entirely during the debounce window (cover_suppressed=true)
    // so rapid j/k keystrokes don't cause expensive pixel-data transmissions.
    let cover_rendered = if cover_suppressed {
        false
    } else {
        match (cover_manager, &track.cover_url) {
            (Some(mgr), Some(url)) => mgr.render_cover(url, cover_area, f),
            _ => false,
        }
    };
    if !cover_rendered {
        draw_cover_placeholder(f, cover_area);
    }

    // Text sub-area: immediately below the cover, no gap.
    let text_y = block_top + cover_h;
    if text_y >= area.y + area.height {
        return;
    }
    let text_area = Rect {
        x: area.x,
        y: text_y,
        width: area.width,
        height: (area.y + area.height).saturating_sub(text_y),
    };

    draw_track_info(f, text_area, track);
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

    // Duration (always present) — append quality only once source is resolved
    let dur = super::format_duration(Some(track.duration));
    let meta = match &track.source {
        Some(src) => format!(
            "⏱  {}  ·  🎵 {}",
            dur,
            format_quality(&src.audio_quality)
        ),
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

    f.render_widget(
        Paragraph::new(lines)
            .alignment(Alignment::Center)
            .wrap(Wrap { trim: true }),
        area,
    );
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
