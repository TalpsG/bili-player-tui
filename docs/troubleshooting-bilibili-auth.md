# Bilibili API 认证与反爬机制调试记录

## 背景

在 P0 阶段开发 `bili-player-cli play BV1hu411M7Ff` 命令时，遇到了一系列与 Bilibili API 认证和反爬机制相关的问题，导致无法获取音频流播放。本文档记录了问题现象、排查过程、尝试过的方案以及最终的解决方案。

---

## 关键名词解释

### SESSDATA

SESSDATA 是 Bilibili 网站的登录凭证（Cookie 中的一个字段）。当你在浏览器登录 B站后，浏览器会保存一个名为 `SESSDATA` 的 Cookie，之后的每次 API 请求都会自动带上这个 Cookie，服务器据此识别你的身份。拥有有效的 SESSDATA 意味着你是"已登录用户"，可以访问更高清的视频、收藏夹等需要登录的功能。

### WBI 签名

WBI（Wbi Border Identification）是 Bilibili 对部分 API 请求增加的签名校验机制。简单来说，它要求请求中必须附带两个额外参数：

- `wts`：当前时间戳
- `w_rid`：根据请求参数和密钥计算出的 MD5 哈希值

如果签名缺失或不正确，API 会拒绝请求。WBI 签名的密钥并非固定值，而是从一个叫做 **nav API** 的接口动态获取的。

### nav API

nav API（`/x/web-interface/nav`）是 Bilibili 的"导航栏"接口，浏览器打开 B站首页时会调用它来获取用户信息（用户名、VIP 状态等）。对我们来说，这个接口的关键作用是：**返回 WBI 签名所需的密钥**（`img_key` 和 `sub_key`，藏在 `data.wbi_img` 字段里）。

### playurl API

playurl API（`/x/player/wbi/playurl`）是获取视频播放地址的接口。给定一个视频的 BV号 和 cid，它会返回该视频的音频流地址、视频流地址等信息。我们播放音频就靠这个接口。

### DASH 格式

DASH（Dynamic Adaptive Streaming over HTTP）是一种流媒体传输格式。B站的高清视频/音频都采用 DASH 格式，音视频是分开的流。我们设置 `fnval=16` 参数来请求 DASH 格式，从而获取独立的音频流。

### Dummy SESSDATA

Dummy SESSDATA 是一个假的 SESSDATA 值（我们用的是字符串 `"dummyval"`）。这个想法来源于 [NoxPlayer](https://github.com/lovegaoshi/NoxPlayer) 项目：当用户没有提供真实的 SESSDATA 时，用一个假值代替，试图绕过 B站的某些反爬检测。

---

## 问题现象

运行 `bili-player-cli play BV1hu411M7Ff` 时，经历了三个阶段的错误：

### 错误 1：获取 WBI 密钥失败（-101）

```
Error: API request failed: -101 - Failed to get WBI keys
```

视频信息获取成功（标题、作者、时长都能显示），但在获取 WBI 密钥时失败。

### 错误 2：修复错误 1 后，获取音频流失败

```
Title: 【4K修复 周杰伦作曲】许茹芸 《手写爱》MV
Error: No audio stream found
```

WBI 密钥获取成功了，视频信息也获取成功了，但音频流为空。

### 错误 3：深入排查发现解析失败

加上调试日志后，发现 API 实际上返回了数据，但 JSON 反序列化失败：

```
[DEBUG] Failed to parse response from https://api.bilibili.com/x/player/wbi/playurl
[DEBUG] Body (first 500 chars): {"code":0,"message":"OK","data":{"dash":{"audio":[...
Error: Parse error: invalid type: null, expected a sequence at line 1 column 34599
```

---

## 排查过程

### 第一步：理解 Bilibili API 的 Cookie 策略

B站的不同 API 对 Cookie 的态度不同：

| API | 不发 Cookie | 发真实 SESSDATA | 发 Dummy SESSDATA |
|-----|-----------|---------------|------------------|
| nav（获取WBI密钥） | 正常返回密钥 | 正常返回密钥+用户信息 | 返回 -101 错误 |
| playurl（获取播放地址） | 正常返回DASH数据 | 返回更高质量音频 | 返回 v_voucher（验证码） |
| search（搜索） | 正常返回搜索结果 | 正常返回搜索结果 | 正常返回搜索结果 |

**关键发现**：Dummy SESSDATA 在 nav API 和 playurl API 上都会引发问题，但在搜索 API 上不会。

### 第二步：nav API 的 -101 问题

nav API 会校验 SESSDATA 的有效性。当它收到一个无效的 `SESSDATA=dummyval` 时，直接返回 -101 错误码（"账号未登录"），并且不返回 WBI 密钥数据。

**不发送任何 Cookie** 时，nav API 虽然也返回非零的 code，但 `data.wbi_img` 字段仍然存在，里面包含我们需要的密钥。

**修复**：nav API 在 SESSDATA 为 dummy 值时不发送 Cookie 头。

### 第三步：playurl API 的 v_voucher 问题

修复 nav API 后，WBI 密钥获取正常了，但 playurl API 仍然返回空数据。通过 curl 直接测试发现：

- **不发送 Cookie**：正常返回 DASH 音频数据（3 条音频流）
- **发送 `Cookie: SESSDATA=dummyval`**：返回 `{"code":0, "data":{"v_voucher":"..."}}` — 只有验证码字段，没有音频数据

`v_voucher` 是 B站的"人机验证"响应：当它检测到异常的登录凭证时，不直接报错，而是要求你完成验证码。这种"静默失败"比返回错误码更难排查，因为 HTTP 状态码和 `code` 字段都是正常的（0 = 成功）。

**修复**：playurl API 同样在 SESSDATA 为 dummy 值时不发送 Cookie 头。

### 第四步：DolbyData 的 null 反序列化问题

修复 Cookie 问题后，playurl API 终于返回了完整的 DASH 数据，但 JSON 反序列化仍然失败：

```
invalid type: null, expected a sequence
```

通过检查 API 原始响应发现，B站对于没有杜比音效的视频，返回的 dolby 字段是这样的：

```json
{
  "dolby": {
    "type": 0,
    "audio": null
  }
}
```

注意：`dolby` 本身不是 `null`（它是一个包含 `type` 和 `audio` 的对象），但 `dolby.audio` 是 `null`。

而我们的 Rust 结构体定义是：

```rust
struct DolbyData {
    audio: Vec<DashAudioItem>,  // 期望一个数组，但收到了 null
}
```

`Vec<DashAudioItem>` 期望接收 `[]` 或 `[...]`，但收到了 `null`，serde 无法将 `null` 反序列化为 `Vec`。

**修复**：将 `audio` 字段改为 `Option<Vec<DashAudioItem>>`，这样 `null` 会被解析为 `None`。

---

## 最终解决方案

### 1. Cookie 策略：只在有真实 SESSDATA 时才发送

```rust
// api.rs - BilibiliClient::get() 方法
if self.has_real_sessdata() {
    request = request.header("Cookie", format!("SESSDATA={}", self.sessdata));
}
```

Dummy SESSDATA（`"dummyval"`）仅作为内部哨兵值（sentinel），用于区分"用户没配置 SESSDATA"和"用户提供了 SESSDATA"，**永远不会作为 Cookie 发送**。

### 2. nav API：接受非零 code 的响应

```rust
// auth.rs - fetch_wbi_keys()
// WBI keys are available even when code != 0 (e.g. not logged in)
let data = nav.data.ok_or_else(|| {
    BilibiliError::Wbi("No data in nav response, cannot get WBI keys".into())
})?;
```

未登录时 nav API 的 `code` 不是 0，但 `data.wbi_img` 仍然存在，所以我们只检查 `data` 和 `wbi_img` 是否存在，不要求 `code == 0`。

### 3. DolbyData.audio：用 Option 包裹 Vec

```rust
// stream.rs
struct DolbyData {
    #[serde(default)]
    audio: Option<Vec<DashAudioItem>>,  // null -> None, [] -> Some([])
}
```

对应的提取逻辑也改为两层 Option 解包：

```rust
if let Some(dolby) = dash.dolby {
    if let Some(audio_list) = dolby.audio {
        if let Some(audio) = audio_list.into_iter().next() {
            // ... 使用 audio
        }
    }
}
```

---

## 教训与总结

1. **静默失败比显式错误更危险**：playurl API 返回 `code: 0` 但数据只有 `v_voucher`，这种"成功响应但缺少关键数据"比返回错误码更难排查。遇到 API 返回"成功"但数据不符合预期时，应该打印原始响应内容检查。

2. **API 响应中的 null 需要防御性处理**：B站 API 的文档不完善，很多字段的 null 行为没有文档说明。对于可能为 null 的字段，Rust 中应优先使用 `Option` 包裹，而不是假设它一定是数组或对象。

3. **不同 API 端点的认证策略不同**：不能假设所有 API 对 Cookie 的处理方式一致。nav API 对无效 Cookie 返回错误码，playurl API 返回验证码要求，而搜索 API 则无所谓。需要逐一测试验证。

4. **Dummy SESSDATA 的局限性**：最初的设想（参考 NoxPlayer）是 dummy SESSDATA 可以绕过 B站的反爬检测，但实际上在 nav 和 playurl 等关键 API 上反而引发了更多问题。最终方案是不发送任何假凭证，让 API 以"未登录"身份正常响应。
