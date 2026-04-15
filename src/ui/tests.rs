use ratatui::{backend::TestBackend, Terminal, buffer::Buffer};
use std::time::Duration;

use crate::app::{App, InputMode, PopupLayer};
use crate::config::Config;
use crate::queue::track::Track;

/// Helper to setup a terminal for testing
fn setup_test_terminal(width: u16, height: u16) -> Terminal<TestBackend> {
    let backend = TestBackend::new(width, height);
    Terminal::new(backend).unwrap()
}

/// Helper to check if buffer contains a string anywhere (ignoring internal spaces for wide-char robustness)
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
        source: None,
    }
}

#[test]
fn test_header_rendering() {
    let mut terminal = setup_test_terminal(80, 24);
    let mut app = App::new(Config::default()).unwrap();
    app.logged_in = true;
    
    terminal.draw(|f| app.draw(f)).unwrap();
    let buffer = terminal.backend().buffer();
    
    assert!(buffer_contains(buffer, "bili-player-cli"));
    assert!(buffer_contains(buffer, "已登录"));
}

#[test]
fn test_responsive_layout() {
    // 3 columns
    let visibility = crate::ui::layout::ColumnVisibility::from_width(100);
    assert!(visibility.playlist && visibility.track_list && visibility.detail);
    
    // 2 columns
    let visibility = crate::ui::layout::ColumnVisibility::from_width(60);
    assert!(visibility.playlist && visibility.track_list && !visibility.detail);
    
    // 1 column
    let visibility = crate::ui::layout::ColumnVisibility::from_width(40);
    assert!(!visibility.playlist && visibility.track_list && !visibility.detail);
}

#[test]
fn test_search_popup_modes() {
    let mut terminal = setup_test_terminal(80, 24);
    let mut app = App::new(Config::default()).unwrap();
    
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
    let mut app = App::new(Config::default()).unwrap();
    
    app.show_popup(PopupLayer::VolumeSlider);
    app.volume = 50;
    
    terminal.draw(|f| app.draw(f)).unwrap();
    assert!(buffer_contains(terminal.backend().buffer(), "Volume: 50%"));
}

#[test]
fn test_help_popup_rendering() {
    let mut terminal = setup_test_terminal(80, 24);
    let mut app = App::new(Config::default()).unwrap();
    
    app.show_popup(PopupLayer::Help);
    terminal.draw(|f| app.draw(f)).unwrap();
    
    assert!(buffer_contains(terminal.backend().buffer(), "Keybindings"));
    assert!(buffer_contains(terminal.backend().buffer(), "Quit"));
}

#[test]
fn test_clear_widget_prevents_bleed() {
    let mut terminal = setup_test_terminal(80, 24);
    let mut app = App::new(Config::default()).unwrap();
    
    // Background text
    app.queue.push(dummy_track("HIDDEN_TEXT"));
    app.show_popup(PopupLayer::Help);
    
    terminal.draw(|f| app.draw(f)).unwrap();
    
    // HIDDEN_TEXT should be covered by help popup and Clear widget
    // Note: It might still exist in areas NOT covered by the popup,
    // so we check if it is NOT present in any line that also contains "Keybindings"
    let buffer = terminal.backend().buffer();
    for y in 0..buffer.area.height {
        let line: String = (0..buffer.area.width)
            .map(|x| buffer[(x, y)].symbol())
            .collect();
        if line.contains("Keybindings") {
            assert!(!line.contains("HIDDEN_TEXT"));
        }
    }
}
