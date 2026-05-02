//! 集成测试：通过 SDK API 测试全部命令（sync 除外）。
//!
//! 使用单个共享 `Yun139Client`，在云盘 `/yun139_sdk_test_<ts>` 目录下依次执行：
//!   1. list     — 列出根目录，验证 API 连通性
//!   2. mkdir    — 创建测试目录（单层 + 递归）
//!   3. upload   — 上传随机小文件（走单次 PUT）
//!   4. upload   — 上传随机大文件（走并行分片）
//!   5. list     — 列出测试目录，验证上传结果
//!   6. search   — 搜索刚上传的文件名
//!   7. download — 单流下载小文件 + SHA256 校验
//!   8. download — 并行下载大文件 + SHA256 校验
//!   9. trash    — 将大文件移入回收站
//!  10. delete   — 永久删除小文件
//!  11. list     — 验证删除结果
//!  12. cleanup  — 删除测试目录
//!
//! 认证方式（按优先级）：
//!   1. 环境变量 `YUN139_AUTH`
//!   2. 系统配置文件 `~/.config/yun139/config.toml`（通过 `yun139 config token` 设置）
//!
//! 运行方式:
//!   cargo test --test integration_all_commands -- --nocapture

use std::io::Write;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

use yun139::{Yun139Client, Yun139Error};
use yun139::config::Config;

// ── 测试配置 ──

/// 小文件：5MB（走单次 PUT 路径）
const SMALL_FILE_SIZE: usize = 5 * 1024 * 1024;
/// 大文件：15MB（走分片上传路径，> 10MB 阈值）
const LARGE_FILE_SIZE: usize = 15 * 1024 * 1024;
/// 下载并发数
const DOWNLOAD_PARALLEL: usize = 4;

// ── 辅助函数 ──

/// 初始化测试日志：输出到终端，不写文件，level = INFO。
/// 使用 try_init 避免多个测试并行时重复初始化报错。
fn init_test_tracing() {
    let _ = tracing_subscriber::fmt()
        .with_max_level(tracing::Level::INFO)
        .with_test_writer()
        .try_init();
}

/// 获取认证信息：优先环境变量，回退到系统配置文件。
fn get_auth() -> Option<String> {
    if let Ok(env) = std::env::var("YUN139_AUTH") {
        if !env.is_empty() {
            return Some(env);
        }
    }
    Config::load().ok().map(|c| c.authorization_header())
}

fn ts_millis() -> u128 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_millis()
}

fn random_seed() -> u64 {
    let t = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos() as u64;
    t ^ (std::process::id() as u64).wrapping_mul(0x9E3779B97F4A7C15)
}

/// 生成随机文件并返回 SHA256 hex。
fn generate_random_file(path: &std::path::Path, size: usize) -> String {
    use sha2::Digest;

    let mut file = std::fs::File::create(path).expect("create random file");
    let mut hasher = sha2::Sha256::new();

    let mut state: u64 = random_seed();
    let chunk_size = 1024 * 1024;
    let mut buf = vec![0u8; chunk_size];
    let mut remaining = size;

    while remaining > 0 {
        let n = remaining.min(chunk_size);
        for byte in buf[..n].iter_mut() {
            state = state
                .wrapping_mul(6364136223846793005)
                .wrapping_add(1442695040888963407);
            *byte = (state >> 33) as u8;
        }
        file.write_all(&buf[..n]).expect("write random data");
        hasher.update(&buf[..n]);
        remaining -= n;
    }

    file.flush().expect("flush random file");
    hex::encode(hasher.finalize())
}

fn sha256_file(path: &std::path::Path) -> String {
    use sha2::Digest;
    use std::io::Read;

    let mut file = std::fs::File::open(path).expect("open file for hash");
    let mut hasher = sha2::Sha256::new();
    let mut buf = vec![0u8; 2 * 1024 * 1024];
    loop {
        let n = file.read(&mut buf).expect("read file");
        if n == 0 {
            break;
        }
        hasher.update(&buf[..n]);
    }
    hex::encode(hasher.finalize())
}

// ── 主测试 ──

#[tokio::test]
async fn sdk_all_commands() {
    init_test_tracing();

    tracing::info!("══════════════════════════════════════════════════════════════════");
    tracing::info!("  sdk_all_commands");
    tracing::info!("  通过 SDK API 全量集成测试，覆盖以下命令：");
    tracing::info!("  list · mkdir · upload(小/大) · list · search ·");
    tracing::info!("  download(单流/并行) · trash · delete · list · error cases");
    tracing::info!("  使用独立时间戳目录隔离，测试结束自动清理云盘与本地临时文件");
    tracing::info!("══════════════════════════════════════════════════════════════════");

    let auth = match get_auth() {
        Some(a) => a,
        None => {
            tracing::warn!("⏭️  跳过: 未设置 YUN139_AUTH 且未找到系统配置 (~/.config/yun139/config.toml)");
            return;
        }
    };
    tracing::info!(
        source = if std::env::var("YUN139_AUTH").is_ok() { "环境变量" } else { "系统配置" },
        "🔑 认证来源"
    );

    let client = Yun139Client::new(&auth).expect("创建 client 失败");

    let ts = ts_millis();
    let test_dir = format!("/yun139_sdk_test_{ts}");
    let sub_dir = format!("{test_dir}/sub/nested");

    let tmp = std::env::temp_dir().join(format!("yun139_sdk_test_{ts}"));
    std::fs::create_dir_all(&tmp).expect("create tmp dir");

    let small_name = format!("small_{ts}.bin");
    let large_name = format!("large_{ts}.bin");
    let small_local = tmp.join(&small_name);
    let large_local = tmp.join(&large_name);

    // 最终清理（即使测试 panic 也尽力执行）
    let cleanup = CleanupGuard {
        client: client.clone(),
        cloud_dir: test_dir.clone(),
        local_dir: tmp.clone(),
    };

    // ═══════════════════════════════════════════════
    // 1. list 根目录 — API 连通性
    // ═══════════════════════════════════════════════
    tracing::info!("── 1. list / ──");
    let root = client.list_all("/").await.expect("list root failed");
    tracing::info!(count = root.items.len(), "根目录条目数");
    // 只验证 API 调用成功（根目录可能为空）

    // ═══════════════════════════════════════════════
    // 2. mkdir — 单层 + 递归
    // ═══════════════════════════════════════════════
    tracing::info!("── 2. mkdir ──");

    // 单层创建测试根目录
    let dir_id = client.mkdir(&test_dir).await.expect("mkdir test_dir failed");
    tracing::info!(dir = %test_dir, file_id = %dir_id, "mkdir");
    assert!(!dir_id.is_empty(), "mkdir 应返回 fileId");

    // 递归创建多层子目录
    let sub_id = client.mkdir_recursive(&sub_dir).await.expect("mkdir_recursive failed");
    tracing::info!(dir = %sub_dir, file_id = %sub_id, "mkdir -r");
    assert!(!sub_id.is_empty(), "mkdir_recursive 应返回 fileId");

    // 验证 list 能看到子目录
    let test_list = client.list_all(&test_dir).await.expect("list test_dir failed");
    let has_sub = test_list.items.iter().any(|i| i.name == "sub" && i.is_folder);
    assert!(has_sub, "test_dir 下应有 sub 目录");

    // ═══════════════════════════════════════════════
    // 3. upload — 小文件 (5MB, 单次 PUT)
    // ═══════════════════════════════════════════════
    tracing::info!(size_mb = SMALL_FILE_SIZE / 1024 / 1024, "── 3. upload small file");
    let small_hash = generate_random_file(&small_local, SMALL_FILE_SIZE);
    tracing::info!(sha256 = %small_hash, "小文件 SHA256");

    let progress = Arc::new(AtomicU64::new(0));
    let p = progress.clone();
    let small_file_id = client
        .upload_file(&small_local, &test_dir, move |uploaded, _total| {
            p.store(uploaded, Ordering::Relaxed);
        })
        .await
        .expect("upload small file failed");
    tracing::info!(file_id = %small_file_id, "小文件上传完成");
    assert!(!small_file_id.is_empty(), "upload 应返回 fileId");
    assert!(
        progress.load(Ordering::Relaxed) > 0,
        "progress 应被调用"
    );

    // ═══════════════════════════════════════════════
    // 4. upload — 大文件 (15MB, 分片上传)
    // ═══════════════════════════════════════════════
    tracing::info!(size_mb = LARGE_FILE_SIZE / 1024 / 1024, "── 4. upload large file");
    let large_hash = generate_random_file(&large_local, LARGE_FILE_SIZE);
    tracing::info!(sha256 = %large_hash, "大文件 SHA256");

    let progress = Arc::new(AtomicU64::new(0));
    let p = progress.clone();
    let large_file_id = client
        .upload_file(&large_local, &test_dir, move |uploaded, _total| {
            p.store(uploaded, Ordering::Relaxed);
        })
        .await
        .expect("upload large file failed");
    tracing::info!(file_id = %large_file_id, "大文件上传完成");
    assert!(!large_file_id.is_empty());

    // ═══════════════════════════════════════════════
    // 5. list — 验证上传结果
    // ═══════════════════════════════════════════════
    tracing::info!("── 5. list test dir ──");
    let dir_list = client.list_all(&test_dir).await.expect("list test_dir failed");
    tracing::info!(count = dir_list.items.len(), "目录条目数");

    let small_found = dir_list.items.iter().find(|i| i.name == small_name);
    let large_found = dir_list.items.iter().find(|i| i.name == large_name);
    assert!(small_found.is_some(), "小文件应出现在列表中");
    assert!(large_found.is_some(), "大文件应出现在列表中");

    let small_item = small_found.unwrap();
    let large_item = large_found.unwrap();
    assert_eq!(small_item.size, SMALL_FILE_SIZE as i64, "小文件 size 应匹配");
    assert_eq!(large_item.size, LARGE_FILE_SIZE as i64, "大文件 size 应匹配");
    assert!(!small_item.is_folder);
    assert!(!large_item.is_folder);

    // ═══════════════════════════════════════════════
    // 6. search — 搜索文件名
    // ═══════════════════════════════════════════════
    tracing::info!("── 6. search ──");
    // 搜索使用时间戳作为关键词，应能命中刚上传的文件
    let search_kw = format!("{ts}");
    let search_result = client.search(&search_kw, 10).await.expect("search failed");
    tracing::info!(keyword = %search_kw, count = search_result.items.len(), "搜索结果");
    // 搜索可能有索引延迟，至少验证 API 调用成功不报错
    let found_small = search_result.items.iter().any(|i| i.name == small_name);
    let found_large = search_result.items.iter().any(|i| i.name == large_name);
    if found_small && found_large {
        tracing::info!("✅ 两个文件都搜到了");
    } else {
        tracing::warn!(found_small, found_large, "⚠️  搜索可能有索引延迟");
    }

    // ═══════════════════════════════════════════════
    // 7. download — 单流下载小文件 + 校验
    // ═══════════════════════════════════════════════
    tracing::info!("── 7. download small (single stream) ──");
    let dl_small = tmp.join(format!("dl_{small_name}"));

    let dl_url = client
        .get_download_url(&small_file_id)
        .await
        .expect("get_download_url small failed");
    assert!(!dl_url.is_empty(), "download URL 不应为空");

    let bytes = client
        .download_single(&dl_url, &dl_small, |_, _| {})
        .await
        .expect("download_single failed");
    assert_eq!(bytes, SMALL_FILE_SIZE as u64, "下载字节数应匹配");

    let dl_small_hash = sha256_file(&dl_small);
    assert_eq!(dl_small_hash, small_hash, "小文件 SHA256 不匹配");
    tracing::info!("✅ 小文件 SHA256 一致");

    // ═══════════════════════════════════════════════
    // 8. download — 并行下载大文件 + 校验
    // ═══════════════════════════════════════════════
    tracing::info!(parallel = DOWNLOAD_PARALLEL, "── 8. download large (parallel) ──");
    let dl_large = tmp.join(format!("dl_{large_name}"));

    let bytes = client
        .download(
            &format!("{test_dir}/{large_name}"),
            dl_large.to_str().unwrap(),
            DOWNLOAD_PARALLEL,
            |_, _| {},
        )
        .await
        .expect("download parallel failed");
    assert_eq!(bytes, LARGE_FILE_SIZE as u64, "下载字节数应匹配");

    let dl_large_hash = sha256_file(&dl_large);
    assert_eq!(dl_large_hash, large_hash, "大文件 SHA256 不匹配");
    tracing::info!("✅ 大文件 SHA256 一致");

    // ═══════════════════════════════════════════════
    // 9. trash — 移入回收站
    // ═══════════════════════════════════════════════
    tracing::info!("── 9. trash large file ──");
    client.trash(&format!("{test_dir}/{large_name}")).await.expect("trash failed");
    tracing::info!("✅ 已移入回收站");

    // 云端有最终一致性延迟，轮询等待大文件从列表消失（最多 5 次，间隔 1s）
    let mut large_still = true;
    for attempt in 1..=5u32 {
        let after_trash = client.list_all(&test_dir).await.expect("list after trash failed");
        large_still = after_trash.items.iter().any(|i| i.name == large_name);
        if !large_still {
            break;
        }
        tracing::info!(attempt, "大文件仍在列表，等待 1s 后重试");
        tokio::time::sleep(std::time::Duration::from_secs(1)).await;
    }
    assert!(!large_still, "大文件应已从列表消失（trash 后等待 5s 仍可见）");

    // ═══════════════════════════════════════════════
    // 10. delete — 永久删除小文件
    // ═══════════════════════════════════════════════
    tracing::info!("── 10. delete small file ──");
    client.delete(&format!("{test_dir}/{small_name}")).await.expect("delete failed");
    tracing::info!("✅ 已永久删除");

    // 同样等待一致性
    let mut file_count = 1usize;
    for attempt in 1..=5u32 {
        let after_delete = client.list_all(&test_dir).await.expect("list after delete failed");
        file_count = after_delete.items.iter().filter(|i| !i.is_folder).count();
        if file_count == 0 {
            tracing::info!(folder_count = after_delete.items.len(), "✅ 无文件残留");
            break;
        }
        tracing::info!(attempt, remaining = file_count, "文件仍在列表，等待 1s 后重试");
        tokio::time::sleep(std::time::Duration::from_secs(1)).await;
    }

    // ═══════════════════════════════════════════════
    // 11. list — 验证全部删除
    // ═══════════════════════════════════════════════
    tracing::info!("── 11. list after delete ──");
    assert_eq!(file_count, 0, "测试目录内应无文件（delete 后等待 5s 仍有残留）");

    // ═══════════════════════════════════════════════
    // 12. 边界 case — 错误路径测试
    // ═══════════════════════════════════════════════
    tracing::info!("── 12. error cases ──");

    // resolve_path 对不存在的路径
    let not_found = client
        .resolve_path(&format!("{test_dir}/no_such_file_ever_{ts}.bin"))
        .await;
    assert!(not_found.is_err(), "不存在路径应返回 Err");
    match not_found.unwrap_err() {
        Yun139Error::PathNotFound(_) => tracing::info!("✅ PathNotFound 正确"),
        other => panic!("期望 PathNotFound, 得到 {other:?}"),
    }

    // download 目录应报错 IsDirectory
    let dl_dir = client
        .download(&test_dir, "/tmp/should_not_exist", 1, |_, _| {})
        .await;
    assert!(dl_dir.is_err(), "下载目录应返回 Err");
    match dl_dir.unwrap_err() {
        Yun139Error::IsDirectory(_) => tracing::info!("✅ IsDirectory 正确"),
        other => panic!("期望 IsDirectory, 得到 {other:?}"),
    }

    // ═══════════════════════════════════════════════
    // 13. cleanup — 永久删除测试目录（不留回收站残留）
    // ═══════════════════════════════════════════════
    tracing::info!("── 13. cleanup ──");
    // 先删叶节点再删父节点，全部永久删除
    let _ = client.delete(&sub_dir).await;
    let _ = client.delete(&format!("{test_dir}/sub")).await;
    let _ = client.delete(&test_dir).await;
    tracing::info!("✅ 云盘测试目录已永久删除");

    // 显式触发 drop 前的清理
    drop(cleanup);

    tracing::info!("🎉 全部命令测试通过！");
}

// ── 清理守卫 ──

/// 测试结束时（包括 panic）清理本地临时文件。
/// 云盘清理在测试流程中显式完成（panic 时无法异步清理）。
struct CleanupGuard {
    #[allow(dead_code)]
    client: Yun139Client,
    #[allow(dead_code)]
    cloud_dir: String,
    local_dir: std::path::PathBuf,
}

impl Drop for CleanupGuard {
    fn drop(&mut self) {
        // 清理本地临时文件
        let _ = std::fs::remove_dir_all(&self.local_dir);
    }
}
