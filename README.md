# bili-player-cli

一个在终端里听 B 站的音频播放器。  
A terminal-based Bilibili audio player — search, browse, and play, all from your shell.

---

![](./assets/screenshot.png)

---

## 灵感来源


- **[Azusa](https://github.com/kenmingwang/azusa-player)** — 浏览器扩展，将 B 站视频转换为纯音频播放，支持歌单管理和歌词搜索。
- **[NoxPlayer](https://github.com/lovegaoshi/NoxPlayer)** — 同样是面向 B 站的浏览器扩展播放器，支持 YouTube，有歌单云备份、音量归一化等功能，并有配套移动端。


---

## 功能

- **三栏 TUI 布局**（Yazi/ranger 风格）：歌单列表 | 曲目列表 | 正在播放
- **搜索 B 站**关键词；或在搜索框粘贴 BV 号 / 完整 URL 获取视频后选择播放
- **完整播放控制**：播放/暂停、上/下一曲、快进快退、音量调节、静音
- **Shuffle & Repeat**：顺序 / 列表循环 / 单曲循环 / 随机四种模式
- **歌单管理**：创建、删除歌单，向歌单添加/移除曲目，重启后持久保留
- **多 P 视频（分P）**：多 P 视频可展开成分P 列表，任意选 P 播放；播某个 P 时会把同集后续分P 顺序入队，`n` 可直接听完一整集
- **封面图显示**：支持 Kitty / Sixel / iTerm2 / Halfblocks 协议

---

## 构建与安装

### 前置依赖

- **Rust (stable, edition 2024)** — 推荐通过 [rustup](https://rustup.rs) 安装
- **libmpv** — 音频后端
  - macOS: `brew install mpv`
  - Linux (Debian/Ubuntu): `apt install libmpv-dev`
  - Linux (Arch): `pacman -S mpv`

### 构建

```bash
git clone https://github.com/TalpsG/bili-player-tui
cd bilibili-player-cli
cargo build --release
# 产物位于: target/release/bili-player-cli
```

### 运行

```bash
# 启动 TUI
./target/release/bili-player-cli

# 直接播放指定视频
./target/release/bili-player-cli play BV1xx411c7mD
```

---

## 快捷键

### 全局

| 按键 | 功能 |
|------|------|
| `q`（主界面） / `Ctrl+C` | 退出程序；弹窗内按各自规则处理 |
| `?` | 打开帮助弹窗 |
| `/` | 打开搜索弹窗 |
| `Space` | 播放 / 暂停 |
| `n` | 下一首 |
| `p` | 上一首 |
| `←` / `→` | 快退 / 快进 5 秒 |
| `m` | 静音切换 |
| `r` | 切换播放模式（▶️ 顺序 → 🔁 列表循环 → 🔂 单曲循环 → 🔀 随机） |
| `Tab` / `Shift+Tab` | 在可见列间切换焦点 |
| `v` | 主界面进入／退出全屏详情（搜索及输入弹窗内不触发） |
| `Enter`（详情列） | 进入全屏详情 |
| `q` / `Esc`（全屏详情） | 返回原焦点；有弹窗（含音量）时先处理弹窗 |
| `Ctrl+L` | 强制重绘（修复 tmux 下图片消失） |

### 列焦点导航

| 按键 | 功能 |
|------|------|
| `h` | 焦点左移一列 |
| `l` | 焦点右移一列 |
| `Enter` | 歌单列：进入歌单（焦点移到曲目列） |

### 光标移动 & 音量

| 按键 | 歌单列 / 曲目列 | 详情列 |
|------|----------------|--------|
| `j` / `↓` | 光标下移 | 音量 -5 |
| `k` / `↑` | 光标上移 | 音量 +5 |
| `J` / `PageDown` | 光标下移 10 行 | — |
| `K` / `PageUp` | 光标上移 10 行 | — |
| `g` / `Home` | 跳到顶部 | — |
| `G` / `End` | 跳到底部 | — |

### 曲目 & 歌单操作

| 按键 | 焦点列 | 功能 |
|------|--------|------|
| `Enter` | 曲目列 | 多 P 视频展开 / 折叠分P；否则播放（歌单视图用整个歌单覆盖队列，并从该行开始播） |
| `e` | 曲目列 | 展开 / 折叠选中视频的分P（与 `Enter` 等价） |
| `a` | 曲目列（歌单视图） | 将选中曲目加入队列（分P 行加该 P，视频行加整集） |
| `A` | 曲目列／搜索结果 | 将选中曲目添加到指定歌单 |
| `c` | 歌单列 | 新建歌单 |
| `d` | 歌单列 | 删除歌单（需二次确认） |
| `d` | 曲目列 | 从队列 / 歌单中移除该曲目（分P 行无效） |

完整按键表见 [KEYBINDINGS.md](./KEYBINDINGS.md)。

### 搜索弹窗

| 按键 | 功能 |
|------|------|
| 输入字符 | INSERT 模式下编辑查询词；`←` / `→`、`Home` / `End` 移动光标 |
| `Enter`（输入框） | 提交关键词或 BV 号 / URL，立即聚焦结果列表 |
| `Esc`（INSERT / 结果列表） | 回到输入框 NORMAL 模式，保留查询和结果 |
| `Esc` / `q`（输入框 NORMAL） | 关闭搜索弹窗 |
| `i` / `/`（NORMAL / 结果列表） | 编辑查询词；输入框 NORMAL 的 `a` 在光标后追加 |
| `j` / `↓` / `Tab`（NORMAL） | 进入结果列表 |
| `j` / `k`、`↓` / `↑`（结果列表） | 上下移动；列表首项向上返回输入框 NORMAL |
| `Enter` / `a` / `A`（结果列表） | 播放 / 加入队列 / 加入指定歌单 |

空查询不会提交；没有结果或请求失败时，若仍聚焦结果列表，则回到输入框 NORMAL。请求期间可按 `Esc` / `i` 返回查询，响应不会抢走焦点。结果列表中的 `q` 也可直接关闭弹窗。

### 歌单弹窗

| 弹窗 | 确认 | 取消 |
|------|------|------|
| 新建歌单 | `Enter` | `Esc` |
| 删除歌单确认 | `Enter` / `y` | `Esc` / `n` / `q` |
| 添加到歌单 | `Enter` | `Esc` / `q` |

---

## 界面与窗口

主界面宽度不足 72 列时仅显示曲目；72–109 列显示歌单和曲目；至少 110 列且 18 行时显示三栏。歌单宽 18–26 列、详情宽 28–40 列，其余空间优先曲目。缩小终端隐藏当前焦点列时，焦点回到曲目列。

全屏详情保留顶栏和播放状态栏，返回时保留列表位置。全屏中的播放、快进／快退、静音和音量快捷键沿用详情列规则，`q` / `Esc` / `v` 返回，列切换键暂不切换底层列表。

软件内置界面文案为英文，视频标题、作者及用户歌单名保留原文。`?` 展示精简基础操作，完整快捷键见 [KEYBINDINGS.md](KEYBINDINGS.md)。帮助弹窗支持 `j/k`、上下方向键逐行滚动，`PageUp/PageDown` 翻页，`Home/End` 跳到首尾；`Esc/q/?` 关闭。

---

## 架构简介

Rust 编写，tokio 异步运行时，ratatui + crossterm 驱动 TUI，libmpv2 作为音频后端，reqwest (rustls) 负责 Bilibili API 请求。`App` 是唯一的状态所有者，主循环通过 `tokio::select!` 串行处理终端输入、mpv 事件和定时 tick。

---

## License

MIT License. Contributions welcome — open an issue or PR.
