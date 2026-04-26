//! 集成测试：通过 CLI 命令测试上传 200MB 随机文件 → 下载 → SHA256 校验
//!
//! 每次运行生成全新随机内容，避免秒传命中，确保真实上传链路被测试。
//! 测试直接调用编译好的 `yun139` 二进制，验证端到端 CLI 工作流。
//!
//! 需要设置环境变量 `YUN139_AUTH` 才能运行。
//!
//! 运行方式:
//!   YUN139_AUTH="Basic cGM6MTM5..." cargo test --test upload_download_roundtrip -- --nocapture

use std::io::Write;
use std::process::Command;
use std::time::{SystemTime, UNIX_EPOCH};

const FILE_SIZE: usize = 200 * 1024 * 1024; // 200MB
const CLOUD_DIR: &str = "/yun139_test_roundtrip";

fn get_auth() -> Option<String> {
    std::env::var("YUN139_AUTH").ok().filter(|s| !s.is_empty())
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
    let auth = match get_auth() {
        Some(a) => a,
        None => {
            eprintln!("⏭️  跳过: 未设置 YUN139_AUTH 环境变量");
            return;
        }
    };

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
    eprintln!("📦 生成 200MB 随机文件 {file_name} ...");
    let original_hash = generate_random_file(&upload_path, FILE_SIZE);
    assert_eq!(
        std::fs::metadata(&upload_path).unwrap().len(),
        FILE_SIZE as u64
    );
    eprintln!("   SHA256: {original_hash}");

    // ── Step 2: CLI upload ──
    eprintln!("⬆️  yun139 upload {} {CLOUD_DIR}", upload_path.display());
    let upload_output = Command::new(&bin)
        .args([
            "--auth",
            &auth,
            "upload",
            upload_path.to_str().unwrap(),
            CLOUD_DIR,
        ])
        .output()
        .expect("执行 upload 命令失败");

    let upload_stderr = String::from_utf8_lossy(&upload_output.stderr);
    let upload_stdout = String::from_utf8_lossy(&upload_output.stdout).trim().to_string();
    eprintln!("{upload_stderr}");

    assert!(
        upload_output.status.success(),
        "❌ upload 退出码非零:\nstderr: {upload_stderr}\nstdout: {upload_stdout}"
    );
    assert!(!upload_stdout.is_empty(), "upload 应输出 fileId");
    eprintln!("   fileId: {upload_stdout}");

    // ── Step 3: CLI download ──
    let cloud_file_path = format!("{CLOUD_DIR}/{file_name}");
    eprintln!(
        "⬇️  yun139 download {cloud_file_path} {}",
        download_path.display()
    );
    let download_output = Command::new(&bin)
        .args([
            "--auth",
            &auth,
            "download",
            &cloud_file_path,
            download_path.to_str().unwrap(),
        ])
        .output()
        .expect("执行 download 命令失败");

    let download_stderr = String::from_utf8_lossy(&download_output.stderr);
    eprintln!("{download_stderr}");

    assert!(
        download_output.status.success(),
        "❌ download 退出码非零:\n{download_stderr}"
    );

    // ── Step 4: 校验 ──
    eprintln!("🔍 校验文件完整性...");
    let dl_size = std::fs::metadata(&download_path).expect("stat download").len();
    assert_eq!(dl_size, FILE_SIZE as u64, "大小不匹配: {dl_size}");

    let downloaded_hash = sha256_file(&download_path);
    eprintln!("   原始 SHA256: {original_hash}");
    eprintln!("   下载 SHA256: {downloaded_hash}");
    assert_eq!(original_hash, downloaded_hash, "SHA256 不匹配!");
    eprintln!("   ✅ SHA256 一致");

    // ── Step 5: 清理 ──
    let _ = std::fs::remove_file(&upload_path);
    let _ = std::fs::remove_file(&download_path);
    eprintln!("🧹 已清理");
    eprintln!("🎉 200MB 随机文件 CLI roundtrip 通过");
}
