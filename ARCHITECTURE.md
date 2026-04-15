# bili-player-cli 架构文档

## 项目定位

命令行 B 站音频播放器。在终端中搜索、浏览、播放 Bilibili 视频的音频流。

## 技术选型

| 方面 | 选择 | 说明 |
|------|------|------|
| 语言 | Rust | 单二进制分发，跨平台 |
| 异步运行时 | tokio | 社区标准 |
| TUI | ratatui + crossterm | 直接使用，不包装 |
| 终端图片 | ratatui-image | 支持 Sixel/Kitty/iTerm2/Halfblocks |
| 音频后端 | AudioBackend trait | libmpv2 |
| HTTP | reqwest (rustls-tls) | 无 OpenSSL 依赖 |
| 配置 | TOML (用户) + JSON (状态/歌单) | |
| CLI | clap (derive) | |
| 日志 | tracing | |

## 模块划分

```
src/
├── main.rs            # 入口
├── lib.rs             # re-exports
├── cli.rs             # CLI 参数
├── app.rs             # App 状态 + 主循环
├── command.rs         # Command enum
├── event.rs           # Event enum
├── config.rs          # 配置加载/保存
├── error.rs           # 错误类型
│
├── bilibili/          # B 站 API 客户端
│   ├── mod.rs
│   ├── api.rs         #   HTTP 请求执行
│   ├── auth.rs        #   Cookie 鉴权 + WBI key 缓存
│   ├── wbi.rs         #   WBI 参数签名
│   ├── search.rs      #   搜索
│   ├── video.rs       #   视频信息
│   ├── stream.rs      #   音频流 URL 解析
│   └── favorite.rs    #   收藏夹
│
├── player/            # 播放后端
│   ├── mod.rs         #   AudioBackend trait + GeneralPlayer
│   ├── mpv.rs         #   libmpv2 实现
│   └── normalize.rs   #   音量归一化 (后续)
│
├── queue/             # 播放队列
│   ├── mod.rs         #   Queue: 列表 + shuffle 排列
│   └── track.rs       #   Track/TrackSource 类型
│
├── playlist/          # 歌单管理
│   ├── mod.rs         #   PlaylistManager
│   └── storage.rs     #   JSON 持久化
│
├── ui/                # TUI 渲染
│   ├── mod.rs         #   Ui struct + draw()
│   ├── layout.rs      #   三栏布局 + 窄终端自适应
│   ├── popup.rs       #   Popup overlay 系统 (z-order, anchor)
│   ├── playlist_view.rs #  左栏: 歌单列表
│   ├── track_list.rs  #   中栏: 曲目列表
│   ├── now_playing.rs #   右栏: 曲目详情 (P2+ 封面图)
│   ├── search_view.rs #   搜索 overlay (popup)
│   ├── help_view.rs   #   帮助 overlay (popup)
│   ├── volume_slider.rs # 音量滑钮 overlay (popup)
│   └── theme.rs       #   暗色主题
│
└── cover/             # 封面图
    ├── mod.rs         #   CoverManager + LRU 缓存
    └── protocol.rs    #   ratatui-image 协议检测
```

## 核心抽象

### AudioBackend trait

播放后端的统一接口。目前只有 libmpv2 实现，但 trait 抽象保留扩展性。`GeneralPlayer` 包装 trait 对象，增加事件发射和跨切逻辑。

```rust
trait AudioBackend: Send + Sync {
    async fn play(&mut self, source: &TrackSource) -> Result<()>;
    fn pause(&mut self);
    fn resume(&mut self);
    fn stop(&mut self);
    fn is_playing(&self) -> bool;
    fn seek(&mut self, offset: Duration) -> Result<()>;
    fn seek_to(&mut self, position: Duration);
    fn position(&self) -> Option<Duration>;
    fn duration(&self) -> Option<Duration>;
    fn set_volume(&mut self, volume: u16) -> u16;
    fn volume(&self) -> u16;
}
```

### TUI 布局系统

Yazi/ranger 风格三栏布局，Painter's algorithm popup overlay。

```
┌─────────────────────────────────────────────────────┐
│ bili-player-cli                          未登录/用户名 │  ← Header (1行)
├──────────┬──────────────────────┬────────────────────┤
│ 歌单列表  │    曲目列表           │   曲目详情          │
│ ▸ 队列   │  ▸ 歌曲A  超爱大可乐  │   歌曲A            │
│          │    歌曲B   周杰伦      │   作者: 超爱大可乐  │
│          │                      │   时长: 4:25       │
│          │                      │   音质: Hi-Res     │
├──────────┴──────────────────────┴────────────────────┤
│ [════════════════════▶               ] 1:23/4:25 🔊80 │  ← Status (1行)
└─────────────────────────────────────────────────────┘
```

**三栏比例**: `Constraint::Ratio(1, 3, 2)` — 歌单 | 曲目 | 详情

**Header**: 左 `bili-player-cli`，右登录状态 (`未登录` / 用户名)

**Status**: 1 行，Gauge 进度条 + 当前时间/总时长 + 🔊音量值

**窄终端自适应**:
- ≥80 列: 三栏 `[1, 3, 2]`
- 50-79 列: 隐藏右栏 `[1, 1, 0]`
- <50 列: 仅中栏 `[0, 1, 0]`

### Popup Overlay 系统

每个 popup 接收全屏区域，自行定位。按 z-order 层叠渲染。

| Z-order | Popup | 触发 | 定位 |
|---------|-------|------|------|
| 0 | 主布局 | 默认 | 全屏 |
| 1 | 音量滑钮 | `↑`/`↓` 调音量 | 状态栏上方居中，自动消失 |
| 2 | 搜索 | `/` | 居中，含输入框+结果列表 |
| 3 | 帮助 | `?` | 全屏 |

**Popup anchor**: 每个 popup 通过 anchor 点 + offset 确定位置 (top-center, center, hovered 等)。

**输入模式**: Normal (默认) → Search Input (按 `/`) → Normal (按 `Esc`/`Enter`)

### 状态所有权

`App` 是唯一的状态所有者。主循环通过 `tokio::select!` 串行处理事件，不需要 `Arc<RwLock>` 散落各处。播放后端在独立线程运行，通过 mpsc channel 传递 `PlayerEvent`。

### 线程模型

- **tokio runtime**: 主循环 (TUI + 事件处理)、API 请求、封面获取
- **mpv 事件线程** (仅 mpv 后端): `mpv.wait_event()` 轮询 → `PlayerEvent` → channel

### B 站 API 调用链

```
搜索: keyword → WBI签名 → /x/web-interface/search/type → Vec<SearchResult>
播放: bvid → /x/web-interface/wbi/view → cid
      bvid+cid → /x/player/wbi/playurl?fnval=16 → DASH音频URL
      URL → mpv loadfile (带 Referer header)
```

## Feature Flags

无 feature flag，libmpv2 为唯一后端。

## 配置文件路径

| OS | 路径 |
|----|------|
| macOS | `~/Library/Application Support/bili-player-cli/` |
| Linux | `~/.config/bili-player-cli/` |
| Windows | `%APPDATA%\bili-player-cli\` |

文件: `config.toml` (用户配置), `state.json` (运行时状态), `playlists.json` (歌单)

---

## 开发周期

### P0: 项目骨架 + B 站 API

**目标**: 打通 B 站 API 全链路，CLI 验证搜索和音频流获取。没有 TUI。

**交付物**: `bili-player-cli play BV1xx...` 可以播放音频

- Cargo 项目初始化 + 所有模块骨架 (空文件/mod.rs)
- `error.rs`: 错误类型定义
- `config.rs`: SESSDATA 配置读取
- `bilibili/`: 完整实现
  - `api.rs`: BilibiliClient (reqwest, Cookie, Referer)
  - `wbi.rs`: mixin key 推导 + MD5 签名
  - `auth.rs`: WBI key 获取与缓存
  - `search.rs`: 关键词搜索
  - `video.rs`: 视频信息 (bvid → cid)
  - `stream.rs`: 音频流 URL (DASH 解析 + 质量选择)
  - `favorite.rs`: 骨架 (P2 实现)
- `queue/track.rs`: Track, TrackSource, AudioQuality 数据类型
- `player/mod.rs`: AudioBackend trait
- `player/mpv.rs`: mpv 后端最简实现 (loadfile + play/pause/stop)
- `cli.rs`: `play` 子命令
- `main.rs`: 入口
- **单元测试**: WBI 签名算法、mixin key 推导、搜索/视频/流解析

**验证**: `cargo test` 全通过 + `bili-player-cli play BV1xx...` 能出声

---

### P1: TUI 框架 + 播放控制

**目标**: 进入 TUI，能搜索、能播、能控制。

**交付物**: TUI 中搜索 → 选曲 → 播放 → 控制完整流程

- `app.rs`: App 状态 + tokio::select! 主循环
- `command.rs`: Command enum
- `event.rs`: Event enum + PlayerEvent
- `ui/` 完整实现:
  - `mod.rs`: Ui struct + draw()，popup z-order 渲染
  - `layout.rs`: 三栏布局 (歌单 | 曲目 | 详情) + Header + Status + 窄终端自适应
  - `popup.rs`: Popup overlay 系统 (anchor 定位 + z-order)
  - `playlist_view.rs`: 左栏歌单列表 (P1 仅"队列")
  - `track_list.rs`: 中栏曲目列表
  - `now_playing.rs`: 右栏曲目详情 (文字，P2+ 封面图)
  - `search_view.rs`: 搜索 overlay (居中 popup，输入框+结果列表)
  - `help_view.rs`: 帮助 overlay (全屏快捷键列表)
  - `volume_slider.rs`: 音量滑钮 overlay (状态栏上方居中，自动消失)
  - `theme.rs`: 基础暗色主题
- `queue/mod.rs`: Queue (顺序播放 + 上下曲)
- `player/mpv.rs`: 完整实现 (mpv 事件线程 + play/pause/seek/volume + position/duration)
- SESSDATA 未配置/无效提示 (Header 右侧)
- 帮助页面 (? 键)
- 所有播放控制快捷键
- **单元测试**: Queue 操作、Command 处理

**验证**: 启动 TUI → 搜索 → 选择 → 播放 → 暂停/seek/音量/上下曲 → 退出

---

### P2: 歌单 + 封面 + 队列增强

**目标**: 能管理曲库，视觉体验提升。

**交付物**: 歌单持久化 + 封面图显示 + Shuffle/Repeat

- `playlist/` 完整实现:
  - `mod.rs`: PlaylistManager (创建/添加/播放/删除歌单)
  - `storage.rs`: JSON 持久化
- `queue/mod.rs`: Shuffle (排列索引) + Repeat (关/列表/单曲)
- `queue` 持久化 (state.json)
- 队列增强: 移除/清空/插入下一首
- `cover/` 实现:
  - `mod.rs`: CoverManager (异步获取 + LRU 缓存)
  - `protocol.rs`: ratatui-image Picker 初始化
- `ui/now_playing.rs`: 封面图显示 (StatefulImage widget)
- `ui/playlist_view.rs`: 左栏歌单列表 (多个歌单 + 队列)
- `bilibili/favorite.rs`: 收藏夹导入 (输入 ID)
- BV 号/URL 直接播放 (搜索框识别)
- **单元测试**: 歌单 CRUD、持久化、Shuffle/Repeat
- **集成测试**: 歌单持久化恢复、收藏夹导入

**验证**: 创建歌单 → 添加曲目 → 重启保留 → 封面显示 → Shuffle/Repeat → 导入收藏夹

---

### P3: 体验打磨

**目标**: 交互细节和边缘场景处理。

- 中文对齐 (unicode-width)
- 窄终端布局自适应完善
- 音量归一化 (player/normalize.rs, mpv `af=lavfi=[loudnorm]`)
- 帮助页面完善 (完整快捷键 + 分组)
- 登录状态显示 + SESSDATA 启动验证
- 网络错误重试 + 状态栏错误提示完善
- 进度条交互 (点击跳转)
- 音量滑钮自动消失定时器

---

### P4: 锦上添花

| 功能 | 说明 |
|------|------|
| 搜索翻页 | 加载更多搜索结果 |
| 搜索历史 | 记录最近搜索词 |
| 快捷键自定义 | 用户可重映射按键 |
| 颜色主题 | dark/light 等主题切换 |
| 导入 UP 主视频列表 | 按 UP 主 mid 批量导入 |
| 重命名歌单 | |
| 歌单导入/导出 | 导出为 JSON 文件，导入从 JSON 文件恢复 |
| MPRIS (Linux) | D-Bus 媒体控制集成 |
| 指定配置路径 | `--config` 参数 |
| 调试模式 | `--debug` 详细日志 |
