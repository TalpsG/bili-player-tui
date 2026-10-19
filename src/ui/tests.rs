use ratatui::{Terminal, backend::TestBackend, buffer::Buffer};
use std::time::Duration;

use crate::app::{App, InputMode, PopupLayer};
use crate::config::Config;
use crate::playlist::Playlist;
use crate::queue::track::Track;

/// Helper to setup a terminal for testing
fn setup_test_terminal(width: u16, height: u16) -> Terminal<TestBackend> {
    let backend = TestBackend::new(width, height);
    Terminal::new(backend).unwrap()
}

/// Helper to check if buffer contains a string anywhere (ignoring internal spaces for wide-char robustness)
/// Column at which `needle` first appears, compared cell by cell.
///
/// Walking cells (rather than concatenating symbols) keeps wide characters from
/// shifting the result: they occupy two cells but contribute one symbol.
fn column_of(buffer: &Buffer, needle: &str) -> Option<usize> {
    let cells: Vec<String> = needle.chars().map(|c| c.to_string()).collect();
    for y in 0..buffer.area.height {
        for x in 0..buffer.area.width.saturating_sub(cells.len() as u16) {
            let hit = cells
                .iter()
                .enumerate()
                .all(|(i, want)| buffer[(x + i as u16, y)].symbol() == want.as_str());
            if hit {
                return Some(x as usize);
            }
        }
    }
    None
}

fn buffer_contains(buffer: &Buffer, pattern: &str) -> bool {
    for y in 0..buffer.area.height {
        let mut line = String::new();
        for x in 0..buffer.area.width {
            let symbol = buffer[(x, y)].symbol();
            if !symbol.is_empty() && symbol != " " {
                line.push_str(symbol);
            }
        }
        // Also clean the pattern for comparison
        let clean_pattern: String = pattern.chars().filter(|&c| c != ' ').collect();
        let clean_line: String = line.chars().filter(|&c| c != ' ').collect();

        if clean_line.contains(&clean_pattern) {
            return true;
        }
    }
    false
}

/// Helper to create a dummy track
fn dummy_track(title: &str) -> Track {
    Track {
        bvid: "BV123".to_string(),
        cid: 12345,
        title: title.to_string(),
        author: "Test Author".to_string(),
        duration: Duration::from_secs(180),
        cover_url: None,
        part: None,
        page: None,
        source: None,
    }
}

fn test_app() -> App {
    App::new_with_playlists(Config::default(), None, Vec::<Playlist>::new()).unwrap()
}

#[test]
fn test_header_rendering() {
    let mut terminal = setup_test_terminal(80, 24);
    let mut app = test_app();
    app.logged_in = true;

    terminal.draw(|f| app.draw(f)).unwrap();
    let buffer = terminal.backend().buffer();

    assert!(buffer_contains(buffer, "bili-player-cli"));
    assert!(buffer_contains(buffer, "Loggedin"));
}

#[test]
fn test_responsive_layout() {
    // 3 columns
    let visibility = crate::ui::layout::ColumnVisibility::from_width(120);
    assert!(visibility.playlist && visibility.track_list && visibility.detail);

    // 2 columns
    let visibility = crate::ui::layout::ColumnVisibility::from_width(80);
    assert!(visibility.playlist && visibility.track_list && !visibility.detail);

    // 1 column
    let visibility = crate::ui::layout::ColumnVisibility::from_width(40);
    assert!(!visibility.playlist && visibility.track_list && !visibility.detail);
}

#[test]
fn test_search_popup_modes() {
    let mut terminal = setup_test_terminal(80, 24);
    let mut app = test_app();

    app.input_mode = InputMode::SearchInput;
    app.show_popup(PopupLayer::Search);
    terminal.draw(|f| app.draw(f)).unwrap();
    assert!(buffer_contains(terminal.backend().buffer(), "INSERT"));

    app.input_mode = InputMode::SearchNormal;
    terminal.draw(|f| app.draw(f)).unwrap();
    assert!(buffer_contains(terminal.backend().buffer(), "NORMAL"));
}

#[test]
fn test_volume_popup_visibility() {
    let mut terminal = setup_test_terminal(80, 24);
    let mut app = test_app();

    app.show_popup(PopupLayer::VolumeSlider);
    app.volume = 50;

    terminal.draw(|f| app.draw(f)).unwrap();
    assert!(buffer_contains(terminal.backend().buffer(), "Volume: 50%"));
}

#[test]
fn test_help_popup_rendering() {
    let mut terminal = setup_test_terminal(80, 24);
    let mut app = test_app();

    app.show_popup(PopupLayer::Help);
    terminal.draw(|f| app.draw(f)).unwrap();

    assert!(buffer_contains(terminal.backend().buffer(), "Help"));
    assert!(buffer_contains(
        terminal.backend().buffer(),
        "main-screen q quits"
    ));
}

#[test]
fn test_clear_widget_prevents_bleed() {
    let mut terminal = setup_test_terminal(80, 24);
    let mut app = test_app();

    // Background text
    app.queue.push(dummy_track("HIDDEN_TEXT"));
    app.show_popup(PopupLayer::Help);

    terminal.draw(|f| app.draw(f)).unwrap();

    // HIDDEN_TEXT should be covered by help popup and Clear widget
    // Note: It might still exist in areas NOT covered by the popup,
    // so we check if it is NOT present in any line that also contains the popup title
    let buffer = terminal.backend().buffer();
    for y in 0..buffer.area.height {
        let line: String = (0..buffer.area.width)
            .map(|x| buffer[(x, y)].symbol())
            .collect();
        if line.contains("Help") {
            assert!(!line.contains("HIDDEN_TEXT"));
        }
    }
}

#[test]
fn test_playlist_list_shows_queue_and_playlists() {
    let mut terminal = setup_test_terminal(80, 24);
    let mut app = test_app();
    app.playlists
        .push(crate::playlist::Playlist::new("My Faves"));
    terminal.draw(|f| app.draw(f)).unwrap();
    let buffer = terminal.backend().buffer();
    assert!(buffer_contains(buffer, "Playlists"));
    assert!(buffer_contains(buffer, "Queue"));
    assert!(buffer_contains(buffer, "MyFaves")); // spaces stripped by buffer_contains helper
}

#[test]
fn test_create_playlist_popup_shown() {
    let mut terminal = setup_test_terminal(80, 24);
    let mut app = test_app();
    app.playlist_name_input = "Rock".to_string();
    app.popup_stack.push(crate::app::PopupLayer::PlaylistCreate);
    terminal.draw(|f| app.draw(f)).unwrap();
    let buffer = terminal.backend().buffer();
    assert!(buffer_contains(buffer, "NewPlaylist"));
}

#[test]
fn test_middle_column_shows_playlist_tracks() {
    let mut terminal = setup_test_terminal(80, 24);
    let mut app = test_app();
    let mut pl = crate::playlist::Playlist::new("Test");
    pl.add_track(dummy_track("Bilibili Hit"));
    app.playlists.push(pl);
    app.playlist_cursor = 1; // focus on that playlist
    terminal.draw(|f| app.draw(f)).unwrap();
    let buffer = terminal.backend().buffer();
    assert!(buffer_contains(buffer, "BilibiliHit"));
}

#[test]
fn test_visible_row_len_queue() {
    let mut app = test_app();
    app.queue.push(dummy_track("A"));
    app.queue.push(dummy_track("B"));
    assert_eq!(app.visible_row_len(), 2);
}

#[test]
fn test_visible_row_len_playlist() {
    let mut app = test_app();
    let mut pl = crate::playlist::Playlist::new("P");
    pl.add_track(dummy_track("X"));
    app.playlists.push(pl);
    app.playlist_cursor = 1;
    assert_eq!(app.visible_row_len(), 1);
}

// ── Multi-part (分P) rendering ───────────────────────────────────────────────

fn parts(n: u32) -> Vec<crate::bilibili::video::VideoPage> {
    (1..=n)
        .map(|i| crate::bilibili::video::VideoPage {
            cid: 1000 + i as u64,
            page: i,
            part: format!("part {i}"),
            duration_secs: 60 + i as u64,
        })
        .collect()
}

/// A multi-part video whose part list is known is marked with `▸`.
#[test]
fn test_multi_part_row_shows_fold_marker() {
    let mut terminal = setup_test_terminal(80, 24);
    let mut app = test_app();
    let track = dummy_track("Collection");
    app.pages_cache.insert(track.bvid.clone(), parts(3));
    app.queue.push(track);

    terminal.draw(|f| app.draw(f)).unwrap();
    assert!(
        buffer_contains(terminal.backend().buffer(), "▸"),
        "a known multi-part video must be marked as foldable"
    );
}

/// Expanded parts render as their own indented rows.
#[test]
fn test_expanded_parts_render_as_rows() {
    let mut terminal = setup_test_terminal(80, 24);
    let mut app = test_app();
    let track = dummy_track("Collection");
    app.pages_cache.insert(track.bvid.clone(), parts(3));
    app.expanded.insert(track.bvid.clone());
    app.queue.push(track);

    assert_eq!(app.visible_row_len(), 4, "1 video row + 3 part rows");

    terminal.draw(|f| app.draw(f)).unwrap();
    let buffer = terminal.backend().buffer();
    assert!(
        buffer_contains(buffer, "▾"),
        "expanded rows use the ▾ marker"
    );
    assert!(buffer_contains(buffer, "P1·part1"));
    assert!(buffer_contains(buffer, "P3·part3"));
}

/// A single-part video gets no fold marker at all.
#[test]
fn test_single_part_row_has_no_marker() {
    let mut terminal = setup_test_terminal(80, 24);
    let mut app = test_app();
    let track = dummy_track("Single");
    app.pages_cache.insert(track.bvid.clone(), parts(1));
    app.queue.push(track);

    terminal.draw(|f| app.draw(f)).unwrap();
    let buffer = terminal.backend().buffer();
    assert!(!buffer_contains(buffer, "▸"));
    assert!(!buffer_contains(buffer, "▾"));
    assert!(buffer_contains(buffer, "Single"));
}

#[test]
fn test_part_rows_are_indented_past_video_rows() {
    let mut terminal = setup_test_terminal(80, 24);
    let mut app = test_app();
    let track = dummy_track("Collection");
    app.pages_cache.insert(track.bvid.clone(), parts(3));
    app.expanded.insert(track.bvid.clone());
    app.queue.push(track);

    terminal.draw(|f| app.draw(f)).unwrap();
    let buffer = terminal.backend().buffer();

    let video_col = column_of(buffer, "Collection").expect("the video row renders its title");
    let part_col = column_of(buffer, "P1 · part 1").expect("the part row renders its part name");
    assert!(
        part_col >= video_col + 4,
        "part rows must indent past video rows (part {part_col}, video {video_col})"
    );
}

#[test]
fn test_queued_parts_are_labelled_with_their_video() {
    let mut terminal = setup_test_terminal(80, 24);
    let mut app = test_app();
    let base = dummy_track("WhiteAlbum");
    let pages = parts(3);
    app.pages_cache.insert(base.bvid.clone(), pages.clone());
    for page in &pages {
        app.queue.push(page.to_track(&base));
    }

    terminal.draw(|f| app.draw(f)).unwrap();
    let buffer = terminal.backend().buffer();

    let title_col = column_of(buffer, "WhiteAlbum - Test Author")
        .expect("the video title labels the run of parts");
    let part_col = column_of(buffer, "P1 · part 1").expect("the parts still render");
    assert!(
        title_col < part_col,
        "the title sits above/left of its parts (title {title_col}, part {part_col})"
    );
}

#[test]
fn test_queued_part_rows_show_part_names() {
    // Entering a multi-part video from a playlist fills the queue with
    // part-level tracks.  They used to render as the video title, so every part
    // of one video looked identical.
    let mut terminal = setup_test_terminal(80, 24);
    let mut app = test_app();
    let base = dummy_track("Collection");
    let pages = parts(3);
    app.pages_cache.insert(base.bvid.clone(), pages.clone());
    app.queue.push(dummy_track("Solo"));
    for page in &pages {
        app.queue.push(page.to_track(&base));
    }

    terminal.draw(|f| app.draw(f)).unwrap();
    let buffer = terminal.backend().buffer();

    for i in 1..=3 {
        assert!(
            buffer_contains(buffer, &format!("P{i}·part{i}")),
            "queued part {i} shows its own name"
        );
    }

    let video_col = column_of(buffer, "Solo").expect("the plain video row renders");
    let part_col = column_of(buffer, "P1 · part 1").expect("the queued part renders");
    assert!(
        part_col >= video_col + 4,
        "queued parts indent like any other part row (part {part_col}, video {video_col})"
    );
}

#[test]
fn constrained_columns_prioritize_tracks_and_respect_sidebar_bounds() {
    use crate::ui::layout::{ColumnVisibility, body_columns};
    use ratatui::layout::Rect;
    for width in [110, 120, 150, 180, 300] {
        let body = Rect::new(3, 4, width, 22);
        let (playlist, tracks, detail) = body_columns(body, ColumnVisibility::from_size(width, 24));
        assert!((18..=26).contains(&playlist.width));
        assert!((28..=40).contains(&detail.width));
        assert!(tracks.width >= 56);
        assert_eq!(playlist.width + tracks.width + detail.width, width);
        assert_eq!(playlist.x, body.x);
        assert_eq!(detail.right(), body.right());
    }
    for (width, height, playlist, detail) in [
        (71, 24, false, false),
        (72, 24, true, false),
        (109, 24, true, false),
        (110, 18, true, true),
        (110, 17, true, false),
        (200, 1, true, false),
    ] {
        let visibility = ColumnVisibility::from_size(width, height);
        assert_eq!((visibility.playlist, visibility.detail), (playlist, detail));
    }
}

#[test]
fn shrinking_terminal_repairs_hidden_focus_and_fullscreen_preserves_lists() {
    use crate::app::FocusColumn;
    let mut terminal = setup_test_terminal(120, 24);
    let mut app = test_app();
    app.queue.push(dummy_track("Selected"));
    app.focus_column = FocusColumn::Detail;
    terminal.draw(|f| app.draw(f)).unwrap();
    assert_eq!(app.focus_column, FocusColumn::Detail);
    app.detail_fullscreen_return_focus = Some(FocusColumn::Detail);
    app.ui.track_list_cursor = 0;
    app.ui.track_list_scroll = 7;
    terminal.backend_mut().resize(60, 10);
    terminal.draw(|f| app.draw(f)).unwrap();
    assert_eq!(app.focus_column, FocusColumn::Detail);
    assert_eq!(app.ui.track_list_scroll, 7);
    app.detail_fullscreen_return_focus = None;
    terminal.draw(|f| app.draw(f)).unwrap();
    assert_eq!(app.focus_column, FocusColumn::TrackList);
}

#[test]
fn responsive_views_render_at_small_and_degenerate_sizes() {
    for (width, height) in [
        (180, 40),
        (110, 18),
        (100, 17),
        (72, 10),
        (40, 8),
        (4, 3),
        (1, 1),
        (0, 0),
    ] {
        let mut terminal = setup_test_terminal(width, height);
        let mut app = test_app();
        app.queue.push(dummy_track("自适应布局 / long title"));
        for fullscreen in [false, true] {
            app.detail_fullscreen_return_focus =
                fullscreen.then_some(crate::app::FocusColumn::TrackList);
            terminal.draw(|f| app.draw(f)).unwrap();
            for popup in [
                PopupLayer::Search,
                PopupLayer::Help,
                PopupLayer::PlaylistCreate,
                PopupLayer::PlaylistDeleteConfirm,
                PopupLayer::AddToPlaylist,
                PopupLayer::VolumeSlider,
            ] {
                app.popup_stack = vec![popup];
                if popup == PopupLayer::Search {
                    app.input_mode = InputMode::SearchInput;
                }
                terminal.draw(|f| app.draw(f)).unwrap();
                app.popup_stack.clear();
                app.input_mode = InputMode::Normal;
            }
        }
    }
}

#[test]
fn all_play_mode_icons_are_visible_in_status_bar() {
    let mut app = App::new_with_playlists(Config::default(), None, Vec::new()).unwrap();
    let mut terminal = setup_test_terminal(80, 24);
    for mode in [
        crate::queue::PlayMode::Sequential,
        crate::queue::PlayMode::RepeatList,
        crate::queue::PlayMode::RepeatOne,
        crate::queue::PlayMode::Shuffle,
    ] {
        app.queue.play_mode = mode;
        terminal
            .draw(|f| crate::ui::draw_status_bar(f, &app, ratatui::layout::Rect::new(0, 23, 80, 1)))
            .unwrap();
        let buffer = terminal.backend().buffer();
        assert!(buffer_contains(buffer, mode.icon()));
        assert!(buffer_contains(buffer, "🔊"));
        for name in ["Sequential", "Repeatlist", "Repeatone", "Shuffle"] {
            assert!(!buffer_contains(buffer, name));
        }
    }
}

#[test]
fn cached_single_part_legacy_leaf_renders_once_without_p1() {
    let mut app = test_app();
    let mut saved = dummy_track("Legacy single video");
    saved.page = Some(1);
    saved.part = Some("Legacy part title".into());
    app.queue.push(saved.clone());
    let page = crate::bilibili::video::VideoPage {
        cid: saved.cid,
        page: 1,
        part: "Legacy part title".into(),
        duration_secs: saved.duration.as_secs(),
    };
    app.pages_cache
        .insert(saved.bvid.clone(), vec![page.clone()]);
    app.expanded.insert(saved.bvid.clone());
    assert_eq!(app.visible_rows().len(), 1);
    let mut terminal = setup_test_terminal(120, 30);
    terminal.draw(|f| app.draw(f)).unwrap();
    assert!(buffer_contains(
        terminal.backend().buffer(),
        "Legacy single video"
    ));
    assert!(!buffer_contains(terminal.backend().buffer(), "P1"));
    assert!(!buffer_contains(
        terminal.backend().buffer(),
        "Legacy part title"
    ));
    assert_eq!(app.queue.tracks()[0].page, Some(1));
    // A real multi-part P1 keeps its group and label.
    let mut second = page.clone();
    second.page = 2;
    second.cid += 1;
    app.pages_cache
        .insert(saved.bvid.clone(), vec![page, second]);
    assert_eq!(app.visible_rows().len(), 2);
    terminal.draw(|f| app.draw(f)).unwrap();
    assert!(buffer_contains(terminal.backend().buffer(), "P1"));
    assert!(buffer_contains(
        terminal.backend().buffer(),
        "Legacy part title"
    ));
}
