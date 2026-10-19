use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::style::{Color, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Clear, List, ListItem};

use crate::app::App;

/// Draw help overlay: centered popup with keybinding reference.
pub fn draw(f: &mut Frame, app: &App, screen: Rect) {
    // Determine size (80% of screen but not too small)
    let width = (screen.width as f32 * 0.8) as u16;
    let height = (screen.height as f32 * 0.8) as u16;

    let area = super::popup::popup_area(
        screen,
        super::popup::Anchor::Center,
        width.max(50),
        height.max(15),
    );

    let keybindings: &[(&str, &str)] = &[
        // ── 全局 ──────────────────────────────────────────────────────────
        ("q / Ctrl+C", "退出程序"),
        ("Ctrl+L", "强制重绘（修复 tmux 切换后图片消失）"),
        ("/", "打开搜索弹窗（支持 BV 号 / B 站链接）"),
        ("?", "打开帮助弹窗（本页面）"),
        (
            "Esc (搜索 INSERT / 结果)",
            "回到输入框 NORMAL；再按 Esc 关闭",
        ),
        (
            "i / / (搜索 NORMAL / 结果)",
            "编辑查询；输入框 Enter 搜索，结果 Enter 播放",
        ),
        ("a / A (搜索结果)", "加入队列 / 加入歌单"),
        ("Tab / Shift+Tab", "在可见列之间循环切换焦点"),
        ("Space", "暂停 / 继续播放"),
        ("n / p", "下一首 / 上一首"),
        ("<- / ->", "快退 / 快进 5 秒（始终有效）"),
        ("m", "静音切换（同时显示音量弹窗）"),
        ("r", "切换播放模式（顺序 → 列表循环 → 单曲循环 → 随机）"),
        // ── 列焦点导航 ────────────────────────────────────────────────────
        ("h", "焦点左移一列"),
        ("l", "焦点右移一列"),
        // ── 光标 ─────────────────────────────────────────────────────────
        ("j/k / down/up (列表列)", "光标下移 / 上移一行"),
        ("j/k / down/up (详情列)", "音量 -5 / +5"),
        ("J/K / PgDn/PgUp", "光标下移 / 上移 10 行"),
        ("g / Home", "跳到列表顶部"),
        ("G / End", "跳到列表底部"),
        // ── 曲目 / 歌单操作 ───────────────────────────────────────────────
        ("Enter (歌单列)", "焦点移到曲目列"),
        ("Enter (队列视图)", "播放选中曲目，多 P 时展开 / 折叠分P"),
        ("Enter (分P 行)", "从该分P 开始播放；歌单视图覆盖队列"),
        ("Enter (歌单视图)", "多 P 时展开 / 折叠，否则覆盖队列播放"),
        ("e (曲目列)", "展开 / 折叠选中视频的分P"),
        ("a", "把选中曲目加入队列（分P 行只加该 P，视频行加整集）"),
        ("A (Shift+A)", "把选中曲目加入指定歌单（分P 行加入该 P）"),
        ("d (队列视图)", "从队列删除选中曲目（分P 行不可删除）"),
        ("d (歌单视图)", "从歌单删除选中曲目（分P 行不可删除）"),
        ("c (歌单列焦点)", "新建歌单（弹窗）"),
        ("d (歌单列焦点)", "删除选中歌单（二次确认弹窗）"),
    ];

    let items: Vec<ListItem> = keybindings
        .iter()
        .map(|&(key, desc)| {
            // Pad key column to a fixed display width using unicode_width.
            // We target 26 display columns for the key field.
            let key_display_width: usize = key
                .chars()
                .map(|c| unicode_width::UnicodeWidthChar::width(c).unwrap_or(1))
                .sum();
            let pad = 26_usize.saturating_sub(key_display_width);
            let padded_key = format!("  {key}{}", " ".repeat(pad));
            let line = Line::from(vec![
                Span::styled(padded_key, Style::default().fg(Color::Cyan)),
                Span::raw(desc),
            ]);
            ListItem::new(line)
        })
        .collect();

    let block = Block::default()
        .borders(Borders::ALL)
        .title(" 快捷键参考（按 q、? 或 Esc 关闭） ")
        .style(app.ui.theme.popup_border);

    let list = List::new(items).block(block);

    // Clear background to avoid overlapping lower-level content
    f.render_widget(Clear, area);
    f.render_widget(list, area);
}
