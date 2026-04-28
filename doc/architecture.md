# yun139 架构设计文档

## 1. 项目概览

**yun139** 是中国移动云盘（139 网盘）的 Rust 命令行客户端及 SDK 库。

- **语言**: Rust (Edition 2021)
- **运行时**: tokio 异步运行时（多线程模式）
- **代码规模**: ~3,600 行（不含测试）
- **双重身份**: 既是可执行的 CLI 工具，也是可作为依赖引入的 SDK 库

### 1.1 核心能力

| 功能 | CLI 命令 | SDK API |
|------|----------|---------|
| 文件上传 | `yun139 upload` | `client.upload()` / `client.upload_file()` |
| 文件下载 | `yun139 download` | `client.download()` / `client.download_parallel()` |
| 目录列表 | `yun139 list` (ls) | `client.list_all()` / `client.list_page()` |
| 创建目录 | `yun139 mkdir` | `client.mkdir()` / `client.mkdir_recursive()` |
| 删除 | `yun139 delete` (rm) | `client.trash()` / `client.delete()` |
| 双向同步 | `yun139 sync` | `client.sync_to_cloud()` / `client.sync_to_local()` |
| 文件搜索 | `yun139 search` | `client.search()` |
| 配置管理 | `yun139 config` | `Config::load()` / `Config::save()` |

### 1.2 版本策略

采用 `0.01.XXXX` 格式，其中 XXXX 为 git commit count，由 `build.rs` 在编译期自动注入。

---

## 2. 系统架构

```
┌─────────────────────────────────────────────────────┐
│                   CLI (main.rs)                     │
│  clap 命令解析 → 子命令分发 → 进度展示 → 退出码     │
└────────────────────────┬────────────────────────────┘
                         │ 调用
┌────────────────────────▼────────────────────────────┐
│                  SDK 库 (lib.rs)                     │
│  顶层便捷函数 download/upload/list/mkdir/...        │
│  (内部创建 client，适合一次性调用)                    │
└────────────────────────┬────────────────────────────┘
                         │
┌────────────────────────▼────────────────────────────┐
│               Yun139Client (client.rs)              │
│  HTTP 客户端 │ 路由发现 │ 路径解析 │ 签名请求        │
│  ┌─────────┐ ┌────────────────┐ ┌──────────────┐    │
│  │ http    │ │ personal_host  │ │ transfer_http│    │
│  │ (30s超时)│ │ (OnceCell缓存) │ │ (无超时)     │    │
│  └─────────┘ └────────────────┘ └──────────────┘    │
└────────────────────────┬────────────────────────────┘
                         │ impl 扩展
┌────────────────────────▼────────────────────────────┐
│              Commands (commands/*.rs)                │
│  download │ upload │ list │ mkdir │ delete │ sync │  │
│  search                                              │
└────────────────────────┬────────────────────────────┘
                         │
┌────────────────────────▼────────────────────────────┐
│              基础设施层                               │
│  api.rs (类型定义)  │ sign.rs (签名)  │ error.rs    │
│  config.rs (配置持久化)                              │
└─────────────────────────────────────────────────────┘
```

### 2.1 模块职责

| 模块 | 文件 | 职责 |
|------|------|------|
| **入口** | `main.rs` | CLI 参数解析、日志初始化、配置读取、命令分发 |
| **SDK 门面** | `lib.rs` | 对外暴露公共类型，提供一次性便捷函数 |
| **客户端核心** | `client.rs` | HTTP 客户端封装、路由发现、路径解析、通用签名请求 |
| **API 类型** | `api.rs` | 所有 API 请求/响应的 serde 结构体，`ApiResponse` trait |
| **签名** | `sign.rs` | mcloud-sign 请求头的计算（MD5 + base64 + encodeURIComponent） |
| **错误** | `error.rs` | 统一错误枚举 `Yun139Error`，涵盖 HTTP/API/IO/路径等 |
| **配置** | `config.rs` | TOML 持久化配置（token、并行数、日志、排除列表） |
| **命令实现** | `commands/*.rs` | 各功能的具体业务逻辑，以 `impl Yun139Client` 形式扩展 |

### 2.2 关键设计决策

1. **`impl Yun139Client` 分散扩展模式**
   - 核心结构体 `Yun139Client` 在 `client.rs` 中定义
   - 各命令模块通过 `impl Yun139Client` 为其添加方法
   - 好处：模块化清晰，避免单文件过大；调用方只需一个 client 实例

2. **双 HTTP 客户端**
   - `http`: 30s 超时，用于轻量 API 调用（路由发现、文件列表等）
   - `transfer_http`: 无超时（仅 connect timeout），用于大文件传输

3. **路由发现 + OnceCell 缓存**
   - 首次请求时通过 `qryRoutePolicy` 获取个人云主机地址
   - `OnceCell` 保证只发现一次，后续请求直接复用
   - 支持 `with_personal_host()` 手动注入（跳过发现，加速冷启动）

4. **Clone 共享**
   - `Yun139Client` 实现 `Clone`（内部全部 Arc/OnceCell）
   - 多个 tokio::spawn 可安全共享同一 client

---

## 3. 依赖关系

```toml
# 核心
reqwest          HTTP 客户端（JSON + stream 特性）
tokio            异步运行时（fs, io-util, sync, rt-multi-thread, macros）
serde/serde_json 序列化
tokio-stream     流式处理（上传 body 包装）

# 安全 & 签名
md-5 / sha2      哈希算法（签名 + 文件校验）
base64           编解码
hex              哈希输出
rand             随机数（签名随机串）

# CLI
clap             命令行解析（derive 模式）
indicatif        进度条（sync 命令）
dirs             跨平台 home 目录

# 配置 & 日志
toml             配置文件解析
tracing*         结构化日志
chrono           时间处理
thiserror        错误类型派生
futures-util     Stream trait 扩展
```

---

## 4. 数据流

### 4.1 认证流

```
浏览器 DevTools → 复制 Authorization Header
                      │
                      ▼
            ┌─ yun139 config token <value> ─┐
            │   base64 解码 → 提取手机号      │
            │   提取过期时间                   │
            │   保存到 ~/.config/yun139/      │
            └─────────────────────────────────┘
                      │
            后续命令自动读取 config.toml
            或从 $YUN139_AUTH 环境变量读取
```

Token 格式：`Basic base64("pc:<phone>:<type>|<flag>|<method>|<expire_ms>|...")`

### 4.2 请求签名流

每个 API 请求都携带 `mcloud-sign` 头，计算流程：

```
JSON body
  │
  ▼ encodeURIComponent
  │
  ▼ 逐字符排序
  │
  ▼ base64 编码
  │
  ▼ MD5 → hash1
  │
  "timestamp:rand16" → MD5 → hash2
  │
  MD5(hash1 + hash2) → 大写 → 最终签名
  │
  ▼
  "timestamp,rand16,SIGN" → mcloud-sign 头
```

---

## 5. 配置系统

配置文件路径：`~/.config/yun139/config.toml`

```toml
authorization = "REDACTED..."  # base64 token
account = "13916407707"                           # 手机号（自动提取）
parallel = 16                                     # 并行传输数
log_level = "warn"                                # trace/debug/info/warn/error/off
log_file = "/path/to/yun139.log"                  # 可选，设置后日志输出到文件
exclude = [".DS_Store", "._*", "Thumbs.db", ...]  # 排除列表
token_expire_time = 1700000000000                  # Token 过期时间戳(ms)
personal_cloud_host = "https://..."                # 可选，缓存主机地址
```

### 5.1 默认排除列表

针对 macOS/Windows 系统文件预设：
`.DS_Store`, `.Spotlight-V100`, `.Trashes`, `.fseventsd`, `.TemporaryItems`,
`Thumbs.db`, `desktop.ini`, `._*`, `.AppleDouble`

### 5.2 日志系统

- 默认输出到 stderr（不干扰管道数据）
- 可配置输出到文件（设置后 stderr 静默，不干扰进度条）
- 日志路径智能解析：目录 → 自动追加 `yun139.log`；有扩展名 → 当作文件
- 支持 `~` 展开

---

## 6. 错误处理

统一错误枚举 `Yun139Error`：

```rust
enum Yun139Error {
    Http(reqwest::Error),        // 网络错误
    Api { code, message },       // API 业务错误
    PathNotFound(String),        // 云盘路径不存在
    IsDirectory(String),         // 期望文件但是目录
    Io(std::io::Error),          // 本地 IO 错误
    Json(serde_json::Error),     // JSON 解析错误
    NoDownloadUrl,               // 响应中无下载链接
    RouteDiscovery(String),      // 路由发现失败
}
```

### 6.1 API 响应统一校验

通过 `ApiResponse` trait + `impl_api_response!` 宏：
- 所有 API 响应结构体实现统一的 `success()`/`code()`/`message()` 方法
- `check()` 方法在 `success == false` 时自动转为 `Yun139Error::Api`
- `post_checked()` = `post_json()` + `check()` 一步到位
