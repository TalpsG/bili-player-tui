# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Project Overview

**bili-player-cli** — a terminal Bilibili audio player in Rust (edition 2024). Searches, browses, and plays audio streams from Bilibili videos via a ratatui TUI. Documentation (ARCHITECTURE.md, SPEC.md) is in Chinese.

## Build & Run

```bash
cargo build                     # Debug build
cargo build --release           # Release build
cargo run                       # Launch TUI
cargo run -- play BV1xxx...     # Direct playback via CLI

# Requires libmpv installed on the system (e.g. `brew install mpv` on macOS)
```

## Testing

```bash
cargo test                      # All tests
cargo test ui::tests            # UI tests only (ratatui TestBackend)
cargo test bilibili::wbi        # WBI signing tests
cargo test queue                # Queue logic tests
```

Tests use `mockito` for HTTP mocking and `tempfile` for filesystem tests. UI tests use `ratatui::backend::TestBackend` with a `buffer_contains()` helper for asserting rendered output.

## Linting & Formatting

```bash
cargo clippy                    # Lint
cargo fmt                       # Format
cargo fmt -- --check            # Check formatting without modifying
```

No custom clippy or rustfmt configuration — uses Rust defaults.

## Architecture

### Event-driven state machine

`App` (app.rs) is the sole state owner. The main loop uses `tokio::select!` to serially process events — no `Arc<RwLock>` needed.

```
Event flow: Terminal input / mpv events / tick → Event enum → Command enum → App::handle_command() → UI re-render
```

- **Thread model**: tokio runtime (TUI + API + async tasks) + dedicated mpv event thread (polls `mpv.wait_event()`, sends `PlayerEvent` over mpsc channel)
- **Tick rate**: 250ms UI refresh

### Module responsibilities

- `bilibili/` — API client: WBI signing (MD5-based), search, video info (bvid→cid), DASH audio stream URL parsing, cookie auth
- `player/` — `AudioBackend` trait + `MpvBackend` implementation (libmpv2). `GeneralPlayer` wraps the trait object, adds event emission
- `queue/` — Playback queue with shuffle (permutation index, preserves original order) and repeat modes (Off/Track/List)
- `playlist/` — Playlist CRUD + JSON persistence (partially implemented)
- `cover/` — Cover image fetching + LRU cache (stub, P2)
- `ui/` — Rendering: three-column layout (Yazi/ranger style), popup overlay system (z-order with painter's algorithm)

### TUI layout system

Three-column ratio `[1, 3, 2]` (playlist | track list | details). Responsive breakpoints:
- ≥80 cols: all three columns
- 50-79 cols: hide detail `[1, 1, 0]`
- <50 cols: track list only `[0, 1, 0]`

Popup overlays (z-order): VolumeSlider (1) → Search (2) → Help (3). Each popup receives full screen area, self-positions via anchor point.

### Input mode state machine

`Normal` → (press `/`) → `SearchInput` → (Esc/Enter) → `SearchNormal` → (Esc) → `Normal`

Focus cycling (`Tab`/`Shift+Tab`) operates on visible columns only.

### Bilibili API call chain

```
Search: keyword → WBI sign → /x/web-interface/search/type → Vec<SearchResult>
Play:   bvid → /x/web-interface/wbi/view → cid
        bvid+cid → /x/player/wbi/playurl?fnval=16 → DASH audio URL
        URL → mpv loadfile (with Referer: https://www.bilibili.com/)
```

All `/wbi/` endpoints require WBI parameter signing (mixin key derived from nav API, MD5 hash). SESSDATA cookie needed for high-quality audio.

## Adding a new feature

1. Add variant to `Command` enum (command.rs)
2. Map key binding in `ui/controls.rs`
3. Implement handler in `App::handle_command()` (app.rs)
4. Update UI rendering in relevant `ui/*.rs` files

## Configuration paths

| OS | Directory |
|----|-----------|
| macOS | `~/Library/Application Support/bili-player-cli/` |
| Linux | `~/.config/bili-player-cli/` |
| Windows | `%APPDATA%\bili-player-cli\` |

Files: `config.toml` (user settings incl. SESSDATA), `state.json` (runtime state), `playlists.json` (playlist data)

## Current development status

Between P1 (TUI + playback controls) and P2 (playlists + covers + shuffle/repeat). See ARCHITECTURE.md for the full P0–P4 roadmap.
