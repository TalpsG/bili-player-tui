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

### User-Agent

User-Agent 是 HTTP 请求头中的一个字段，用于标识发起请求的客户端类型（如浏览器、命令行工具等）。B站的部分 API 会检查 User-Agent，对看起来不像浏览器的请求（如 `curl/8.7.1`）返回空数据或触发风控。

### try_look

`try_look=1` 是 playurl API 的一个参数，允许未登录用户"试看"视频。参考 [NoxPlayer](https://github.com/lovegaoshi/NoxPlayer) 项目，该参数可以在不登录的情况下获取音频流。

---

## 问题现象

运行 `bili-player-cli play BV1hu411M7Ff` 时，经历了多个阶段的错误：

### 错误 1：获取 WBI 密钥失败（-101）

```
Error: API request failed: -101 - Failed to get WBI keys
```

视频信息获取成功（标题、作者、时长都能显示），但在获取 WBI 密钥时失败。

### 错误 2：修复错误 1 后，获取音频流失败（间歇性）

```
Title: 【4K修复 周杰伦作曲】许茹芸 《手写爱》MV
Error: No audio stream found
```

WBI 密钥获取成功了，视频信息也获取成功了，但音频流为空。**关键特征：间歇性出现**——有时能播放，有时不能，同样的代码同样的命令结果不一致。

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

B站的不同 API 对无效 Cookie 的态度不同：

| API | 不发 Cookie | 发真实 SESSDATA | 发假 SESSDATA |
|-----|-----------|---------------|--------------|
| nav（获取WBI密钥） | 正常返回密钥 | 正常返回密钥+用户信息 | 返回 -101 错误 |
| playurl（获取播放地址） | 正常返回DASH数据 | 返回更高质量音频 | 返回 v_voucher（验证码） |
| search（搜索） | 正常返回搜索结果 | 正常返回搜索结果 | 正常返回搜索结果 |

**关键发现**：假 SESSDATA 在 nav API 和 playurl API 上都会引发问题，但在搜索 API 上不会。而实测表明搜索 API 不发 Cookie 也能正常工作，所以根本没有发送假 Cookie 的必要。

### 第二步：nav API 的 -101 问题

最初代码参考了 [NoxPlayer](https://github.com/lovegaoshi/NoxPlayer) 的做法，在用户未提供 SESSDATA 时用一个假值 `"dummyval"` 作为 Cookie 发送。nav API 会校验 SESSDATA 的有效性——当它收到一个无效的 `SESSDATA=dummyval` 时，直接返回 -101 错误码（"账号未登录"），并且不返回 WBI 密钥数据。

**不发送任何 Cookie** 时，nav API 虽然也返回非零的 code，但 `data.wbi_img` 字段仍然存在，里面包含我们需要的密钥。

**修复**：nav API 在用户未登录时不发送 Cookie 头。

### 第三步：playurl API 的 v_voucher 问题

修复 nav API 后，WBI 密钥获取正常了，但 playurl API 仍然返回空数据。通过 curl 直接测试发现：

- **不发送 Cookie**：正常返回 DASH 音频数据（3 条音频流）
- **发送 `Cookie: SESSDATA=dummyval`**：返回 `{"code":0, "data":{"v_voucher":"..."}}` — 只有验证码字段，没有音频数据

`v_voucher` 是 B站的"人机验证"响应：当它检测到异常的登录凭证时，不直接报错，而是要求你完成验证码。这种"静默失败"比返回错误码更难排查，因为 HTTP 状态码和 `code` 字段都是正常的（0 = 成功）。

**修复**：playurl API 同样在用户未登录时不发送 Cookie 头。

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

### 第五步：User-Agent 导致的间歇性失败

修复上述问题后，play 命令仍然**间歇性**失败。添加诊断输出发现：

```
[DBG] playurl code=0, has_data=true, has_dash=false
[DBG] data fields present: dash=false, durl=false
```

API 返回 `code: 0` 和 `data`，但 `data.dash` 和 `data.durl` 都不存在。通过 curl 对比测试：

| User-Agent | 结果 |
|-----------|------|
| `curl/8.7.1`（curl 默认） | `has_dash: false` — 被拦截 |
| `reqwest/0.12.12`（reqwest 默认） | `has_dash: true` — 通过 |
| Firefox UA | `has_dash: true` — 通过 |

**根因**：B站 playurl API 会检查 User-Agent，对非浏览器的 UA 返回空数据（同样是 `code: 0` 的静默失败）。reqwest 的默认 UA 偶尔能通过，但 B站的反爬策略并非每次都严格执行，导致了间歇性失败。

**修复**：在所有 API 请求中伪装浏览器 User-Agent。

### 第六步：WBI 签名缺少 URL 编码

根据 [bilibili-API-collect](https://github.com/SocialSisterYi/bilibili-API-collect) 文档，WBI 签名算法要求对参数值做 `encodeURIComponent` 风格的 URL 编码（大写十六进制，空格为 `%20`）。我们的原始实现没有做 URL 编码，对于纯 ASCII 参数（如 `bvid=BV1hu411M7Ff`）不会出错，但搜索 API 的 `keyword` 参数包含中文时签名会错误。

**修复**：实现 `encodeURIComponent` 函数，在计算 w_rid 时对参数进行 URL 编码。

### 第七步：添加 try_look=1 参数

参考 [NoxPlayer](https://github.com/lovegaoshi/NoxPlayer) 的实现，playurl API 加上 `try_look=1` 参数可以允许未登录用户预览/获取音频流，提高无登录情况下的成功率。

---

## 尝试过的方案

### 方案 A：用假 SESSDATA 绕过反爬（已放弃）

最初参考 NoxPlayer 项目，在用户没有 SESSDATA 时发送一个假的 `"dummyval"` 作为 Cookie。初衷是假设搜索 API 在完全没有 Cookie 时会返回 412（风控拦截），但实测发现：

1. 搜索 API 不发 Cookie 也能正常工作
2. 假 SESSDATA 在 nav API 上触发 -101 错误
3. 假 SESSDATA 在 playurl API 上触发 v_voucher（人机验证）

结论：假 Cookie 弊大于利，完全不需要。

### 方案 B：不同接口使用不同 Cookie 策略（已放弃）

第二个方案是统一发送 Cookie，但在 nav API 等特定端点跳过假值。这通过硬编码 `"dummyval"` 字符串比较来实现：

```rust
if !sessdata.is_empty() && sessdata != "dummyval" {
    request = request.header("Cookie", format!("SESSDATA={sessdata}"));
}
```

这个方案能工作，但用 magic string 做哨兵值不优雅，而且逻辑散落在多处。

### 方案 C：关闭 cookie_store（已尝试，非根因）

怀疑 reqwest 的 `cookie_store(true)` 导致请求间状态污染，自动回传了 B站的风控 Cookie。关闭后问题依然存在，说明不是根因。

### 方案 D：用 Option<String> 表示可选的 SESSDATA + 浏览器 UA（最终方案）

去掉 dummy 值，用 Rust 的 `Option<String>` 类型表达"有或没有 SESSDATA"，同时在所有请求中伪装浏览器 User-Agent，加上 `try_look=1` 参数。

---

## 最终解决方案

### 1. SESSDATA 用 Option<String> 表示，不发任何假凭证

```rust
// api.rs
pub struct BilibiliClient {
    sessdata: Option<String>,  // None = 未登录，Some = 已登录
    // ...
}

// get() 方法中
if let Some(ref sessdata) = self.sessdata {
    request = request.header("Cookie", format!("SESSDATA={sessdata}"));
}
```

### 2. 伪装浏览器 User-Agent

```rust
// api.rs - get() 方法中
request = request.header(
    "User-Agent",
    "Mozilla/5.0 (X11; Linux x86_64; rv:128.0) Gecko/20100101 Firefox/128.0",
);
```

### 3. playurl 请求添加 try_look=1

```rust
// stream.rs
let params = vec![
    ("bvid".to_string(), bvid.to_string()),
    ("cid".to_string(), cid.to_string()),
    ("qn".to_string(), "64".to_string()),
    ("fnval".to_string(), "16".to_string()),
    ("try_look".to_string(), "1".to_string()),
];
```

### 4. WBI 签名添加 URL 编码

```rust
// wbi.rs - encodeURIComponent 风格编码
fn encodeURIComponent(s: &str) -> String {
    let mut result = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            'A'..='Z' | 'a'..='z' | '0'..='9' | '-' | '_' | '.' | '~' => {
                result.push(c);
            }
            _ => {
                let mut buf = [0u8; 4];
                for byte in c.encode_utf8(&mut buf).as_bytes() {
                    result.push_str(&format!("%{byte:02X}"));
                }
            }
        }
    }
    result
}
```

### 5. nav API：接受非零 code 的响应

```rust
// auth.rs - fetch_wbi_keys()
// WBI keys are available even when code != 0 (e.g. not logged in)
let data = nav.data.ok_or_else(|| {
    BilibiliError::Wbi("No data in nav response, cannot get WBI keys".into())
})?;
```

### 6. DolbyData.audio：用 Option 包裹 Vec

```rust
// stream.rs
struct DolbyData {
    #[serde(default)]
    audio: Option<Vec<DashAudioItem>>,  // null -> None, [] -> Some([])
}
```

---

## 教训与总结

1. **静默失败比显式错误更危险**：playurl API 返回 `code: 0` 但数据只有 `v_voucher` 或空 dash，这种"成功响应但缺少关键数据"比返回错误码更难排查。遇到 API 返回"成功"但数据不符合预期时，应该打印原始响应内容检查。

2. **API 响应中的 null 需要防御性处理**：B站 API 的文档不完善，很多字段的 null 行为没有文档说明。对于可能为 null 的字段，Rust 中应优先使用 `Option` 包裹，而不是假设它一定是数组或对象。

3. **不同 API 端点的认证策略不同**：不能假设所有 API 对 Cookie 的处理方式一致。nav API 对无效 Cookie 返回错误码，playurl API 返回验证码要求，而搜索 API 则无所谓。需要逐一测试验证。

4. **不要用 magic string 替代类型系统**：最初用 `"dummyval"` 字符串作为哨兵值区分"有没有 SESSDATA"，这本质上是在用字符串模拟 `Option` 类型。Rust 的 `Option<String>` 已经完美表达了"有或没有"的语义，应该直接使用类型系统而非引入 magic string。

5. **User-Agent 是反爬的第一道防线**：B站不仅检查 Cookie，还检查 User-Agent。对于命令行工具，必须伪装浏览器 UA，否则部分 API 会静默返回空数据。这也解释了为什么用 curl 测试时总是失败（curl 默认 UA 是 `curl/x.x.x`），而程序里有时能成功（reqwest 默认 UA 有时能通过 B站的风控）。

6. **间歇性故障通常意味着反爬策略的模糊执行**：B站的反爬并非每次都严格执行（可能是基于请求频率、时间段、IP 信誉等因素的动态策略），导致同样的代码有时能通过有时不能。解决方案应该是满足所有已知的反爬要求，而不是依赖"有时候能通过"。
