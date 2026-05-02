//! 集成测试：通过 CLI 命令测试上传 200MB 随机文件 → 下载 → SHA256 校验
//!
//! 每次运行生成全新随机内容，避免秒传命中，确保真实上传链路被测试。
//! 测试直接调用编译好的 `yun139` 二进制，验证端到端 CLI 工作流。
//! 测试正常完成后云盘无残留（永久删除上传文件）。
//!
//! 认证方式（按优先级）：
//!   1. 环境变量 `YUN139_AUTH`
//!   2. 系统配置文件 `~/.config/yun139/config.toml`（通过 `yun139 config token` 设置）
//!
//! 运行方式:
//!   cargo test --test upload_download_roundtrip -- --nocapture

use std::io::Write;
use std::process::Command;
use std::time::{SystemTime, UNIX_EPOCH};

use yun139::config::Config;

const FILE_SIZE: usize = 200 * 1024 * 1024; // 200MB
const CLOUD_DIR: &str = "/yun139_test_roundtrip";

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

/// 获取 CLI 二进制路径（cargo test 编译产物同目录）
fn cli_bin() -> std::path::PathBuf {
    let mut path = std::env::current_exe().expect("current_exe");
    // tests binary 在 target/debug/deps/xxx, CLI 在 target/debug/yun139
    path.pop(); // deps/
    path.pop(); // debug/
    path.push("yun139");
    path
}

fn random_seed() -> u64 {
    let t = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos() as u64;
    t ^ (std::process::id() as u64).wrapping_mul(0x9E3779B97F4A7C15)
}

fn generate_random_file(path: &std::path::Path, size: usize) -> String {
    use sha2::Digest;

    let mut file = std::fs::File::create(path).expect("create test file");
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
        file.write_all(&buf[..n]).expect("write test file");
        hasher.update(&buf[..n]);
        remaining -= n;
    }

    file.flush().expect("flush test file");
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

#[test]
fn upload_download_roundtrip_via_cli() {
    init_test_tracing();

    tracing::info!("══════════════════════════════════════════════════════════════");
    tracing::info!("  upload_download_roundtrip_via_cli");
    tracing::info!("  通过 CLI 命令上传 200MB 随机文件，下载后 SHA256 校验完整性");
    tracing::info!("  流程：生成随机文件 → CLI upload → CLI download → 校验哈希");
    tracing::info!("══════════════════════════════════════════════════════════════");

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

    let bin = cli_bin();
    assert!(bin.exists(), "CLI 未编译: {:?}，请先 cargo build", bin);

    let tmp_dir = std::env::temp_dir().join("yun139_test");
    std::fs::create_dir_all(&tmp_dir).expect("create tmp dir");

    let ts = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_millis();
    let file_name = format!("rnd_{ts}.bin");
    let upload_path = tmp_dir.join(&file_name);
    let download_path = tmp_dir.join(format!("dl_{ts}.bin"));

    // ── Step 1: 生成 200MB 随机文件 ──
    tracing::info!(file = %file_name, "📦 生成 200MB 随机文件");
    let original_hash = generate_random_file(&upload_path, FILE_SIZE);
    assert_eq!(
        std::fs::metadata(&upload_path).unwrap().len(),
        FILE_SIZE as u64
    );
    tracing::info!(sha256 = %original_hash, "   原始 SHA256");

    // ── Step 2: CLI upload ──
    tracing::info!(src = %upload_path.display(), dest = CLOUD_DIR, "⬆️  CLI upload");
    let upload_output = Command::new(&bin)
        .env("YUN139_AUTH", &auth)
        .args([
            "upload",
            upload_path.to_str().unwrap(),
            CLOUD_DIR,
        ])
        .output()
        .expect("执行 upload 命令失败");

    let upload_stderr = String::from_utf8_lossy(&upload_output.stderr);
    let upload_stdout = String::from_utf8_lossy(&upload_output.stdout).trim().to_string();
    if !upload_stderr.trim().is_empty() {
        tracing::info!("{}", upload_stderr.trim());
    }

    assert!(
        upload_output.status.success(),
        "❌ upload 退出码非零:\nstderr: {upload_stderr}\nstdout: {upload_stdout}"
    );
    assert!(!upload_stdout.is_empty(), "upload 应输出 fileId");
    tracing::info!(file_id = %upload_stdout, "   fileId");

    // ── Step 3: CLI download ──
    let cloud_file_path = format!("{CLOUD_DIR}/{file_name}");
    tracing::info!(src = %cloud_file_path, dest = %download_path.display(), "⬇️  CLI download");
    let download_output = Command::new(&bin)
        .env("YUN139_AUTH", &auth)
        .args([
            "download",
            &cloud_file_path,
            download_path.to_str().unwrap(),
        ])
        .output()
        .expect("执行 download 命令失败");

    let download_stderr = String::from_utf8_lossy(&download_output.stderr);
    if !download_stderr.trim().is_empty() {
        tracing::info!("{}", download_stderr.trim());
    }

    assert!(
        download_output.status.success(),
        "❌ download 退出码非零:\n{download_stderr}"
    );

    // ── Step 5: 校验 ──
    tracing::info!("🔍 校验文件完整性");
    let dl_size = std::fs::metadata(&download_path).expect("stat download").len();
    assert_eq!(dl_size, FILE_SIZE as u64, "大小不匹配: {dl_size}");

    let downloaded_hash = sha256_file(&download_path);
    tracing::info!(original = %original_hash, downloaded = %downloaded_hash, "SHA256 对比");
    assert_eq!(original_hash, downloaded_hash, "SHA256 不匹配!");
    tracing::info!("   ✅ SHA256 一致");

    // ── Step 6: 云盘清理（永久删除文件 + 目录）──
    let cloud_file_path_del = format!("{CLOUD_DIR}/{file_name}");
    tracing::info!(path = %cloud_file_path_del, "🗑️  CLI delete --permanent (file)");
    let del_file = Command::new(&bin)
        .env("YUN139_AUTH", &auth)
        .args(["delete", "--permanent", &cloud_file_path_del])
        .output()
        .expect("执行 delete 命令失败");
    if del_file.status.success() {
        tracing::info!("✅ 云盘文件已永久删除");
    } else {
        tracing::warn!(
            stderr = %String::from_utf8_lossy(&del_file.stderr),
            "⚠️  云盘文件删除失败（不影响测试结果）"
        );
    }

    // 删除测试目录本身（无论空不空）
    tracing::info!(path = CLOUD_DIR, "🗑️  CLI delete --permanent (dir)");
    let del_dir = Command::new(&bin)
        .env("YUN139_AUTH", &auth)
        .args(["delete", "--permanent", CLOUD_DIR])
        .output()
        .expect("执行 delete 命令失败");
    if del_dir.status.success() {
        tracing::info!("✅ 云盘目录已永久删除");
    } else {
        tracing::warn!(
            stderr = %String::from_utf8_lossy(&del_dir.stderr),
            "⚠️  云盘目录删除失败（不影响测试结果）"
        );
    }

    // ── Step 7: 清理本地临时文件 ──
    let _ = std::fs::remove_file(&upload_path);
    let _ = std::fs::remove_file(&download_path);
    tracing::info!("🧹 本地临时文件已清理");
    tracing::info!("🎉 200MB 随机文件 CLI roundtrip 通过");
}
