# yun139 SDK 开发指南

## 1. 快速上手

```rust
use yun139::Yun139Client;

#[tokio::main]
async fn main() -> yun139::Result<()> {
    let client = Yun139Client::new("Basic <YOUR_TOKEN>")?;

    // 列出根目录
    let items = client.list_all("/").await?;
    for item in &items.items {
        println!("{} ({})", item.name, if item.is_folder { "folder" } else { "file" });
    }

    Ok(())
}
```

## 2. Client 创建

```rust
// 方式 1: 直接传入 token
let client = Yun139Client::new("Basic xxx")?;

// 方式 2: 仅传 base64 部分
let client = Yun139Client::new("cGM6MTM5...")?;

// 方式 3: 指定 personal_host（跳过路由发现，更快）
let client = Yun139Client::new("Basic xxx")?
    .with_personal_host("https://personal-kd-njs.yun.139.com/hcy");

// Client 是 Clone 的，可多 spawn 共享
let c1 = client.clone();
tokio::spawn(async move { c1.list_all("/").await });
```

## 3. 便捷函数

`lib.rs` 提供无需创建 client 的一次性函数：

```rust
// 下载
let bytes = yun139::download("Basic xxx", "/file.mp4", "./file.mp4", 4, |w, t| {}).await?;

// 上传
let file_id = yun139::upload("Basic xxx", "/backup", "./data.zip", |u, t| {}).await?;

// 列表
let result = yun139::list("Basic xxx", "/").await?;

// 创建目录
let dir_id = yun139::mkdir("Basic xxx", "/a/b/c", true).await?;

// 删除
yun139::trash("Basic xxx", "/old.txt").await?;
yun139::delete("Basic xxx", "/old.txt").await?;

// 搜索
let result = yun139::search("Basic xxx", "photo", 50).await?;

// 同步
let summary = yun139::sync_to_cloud("Basic xxx", &path, "/backup", false, |_| {}).await?;
let summary = yun139::sync_to_local("Basic xxx", "/backup", &path, false, |_| {}).await?;
```

## 4. 进度回调

### 上传

```rust
client.upload_file(&path, "/backup", |uploaded_bytes, total_bytes| {
    let pct = uploaded_bytes as f64 / total_bytes as f64 * 100.0;
    eprint!("\r{:.1}%", pct);
}).await?;
```

### 下载

```rust
client.download("/file.mp4", "./file.mp4", 4, |written_bytes, total_size| {
    // total_size: Option<u64>，某些场景可能为 None
    if let Some(total) = total_size {
        eprint!("\r{}/{}", written_bytes, total);
    }
}).await?;
```

## 5. Sync 高级配置

```rust
use yun139::{SyncOptions, SyncDirection};

let opts = SyncOptions::default()
    .with_concurrency(8)       // 并发数
    .with_delete(true)         // 删除目标多余文件
    .with_upload_only(false)   // 仅上传
    .with_download_only(false); // 仅下载

let summary = client
    .sync_to_cloud_with_options(&local_dir, "/backup", &opts, |msg| {
        println!("{msg}");
    })
    .await?;

println!("上传: {}, 下载: {}, 失败: {}", summary.uploaded, summary.downloaded, summary.failed);
```

## 6. 错误处理

```rust
use yun139::{Yun139Error, Result};

match client.download("/no_such_file", "./out", 1, |_, _| {}).await {
    Ok(bytes) => println!("下载 {bytes} 字节"),
    Err(Yun139Error::PathNotFound(path)) => eprintln!("路径不存在: {path}"),
    Err(Yun139Error::IsDirectory(path)) => eprintln!("是目录: {path}"),
    Err(Yun139Error::Http(e)) => eprintln!("网络错误: {e}"),
    Err(Yun139Error::Api { code, message }) => eprintln!("API 错误 [{code}]: {message}"),
    Err(e) => eprintln!("其他错误: {e}"),
}
```

## 7. 配置读取

```rust
use yun139::config::Config;

// 从 ~/.config/yun139/config.toml 加载
let config = Config::load()?;
println!("账号: {}, 并行: {}", config.account, config.parallel);

// 从 token 构造
let config = Config::from_token("Basic xxx")?;
config.save()?;

// 检查过期
if config.is_expired() {
    eprintln!("Token 即将过期: {}", config.expire_time_display());
}
```

## 8. 公开类型一览

| 类型 | 模块 | 说明 |
|------|------|------|
| `Yun139Client` | `client` | 核心客户端（Clone + Send + Sync） |
| `Yun139Error` | `error` | 错误枚举 |
| `Result<T>` | `error` | `std::result::Result<T, Yun139Error>` |
| `ListItem` | `commands::list` | 列表条目 |
| `ListResult` | `commands::list` | 列表结果 |
| `SearchResult` | `commands::search` | 搜索结果 |
| `SyncAction` | `commands::sync` | 同步动作枚举 |
| `SyncDirection` | `commands::sync` | 同步方向 |
| `SyncOptions` | `commands::sync` | 同步选项 |
| `SyncSummary` | `commands::sync` | 同步执行摘要 |
| `Config` | `config` | 配置结构 |

## 9. 线程安全

`Yun139Client` 内部全部使用 `Arc` / `OnceCell`，实现了 `Clone`：
- 多个 `tokio::spawn` 可安全共享同一个 client
- 路由发现结果自动缓存（`OnceCell`），不会重复请求
- 无内部可变状态（无 Mutex 争用）
