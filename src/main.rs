use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;

use clap::{CommandFactory, FromArgMatches, Parser, Subcommand};

/// 格式: 0.01.XXXX (主版本.子版本.提交次数)
fn version_string() -> &'static str {
    // commit count 由 build.rs 在编译时注入
    const COMMIT_COUNT: &str = env!("GIT_COMMIT_COUNT");

    // 用 const + macro 无法拼接，运行时 leak 一次即可
    static VERSION: std::sync::OnceLock<String> = std::sync::OnceLock::new();
    VERSION.get_or_init(|| {
        format!("0.01.{:0>4}", COMMIT_COUNT)
    })
}

/// 中国移动云盘 (139网盘) CLI
#[derive(Parser)]
#[command(
    name = "yun139-cli",
    about = "139 云盘命令行工具",
)]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// 管理配置（token、并行数、日志等）
    #[command(alias = "cfg")]
    Config {
        #[command(subcommand)]
        action: ConfigAction,
    },

    /// 上传本地文件到云盘
    Upload {
        /// 本地文件路径
        local_path: String,
        /// 云盘目标目录，如 /backup 或 /
        #[arg(default_value = "/")]
        cloud_dir: String,
    },

    /// 从云盘下载文件到本地
    Download {
        /// 云盘文件路径，如 /backup/test.mp4
        cloud_path: String,
        /// 本地保存路径
        local_path: String,
    },

    /// 列出云盘目录内容
    #[command(alias = "ls")]
    List {
        /// 云盘目录路径
        #[arg(default_value = "/")]
        cloud_dir: String,
    },

    /// 创建云盘目录
    Mkdir {
        /// 云盘目录路径，如 /photos/2024
        cloud_path: String,
        /// 递归创建，自动创建中间目录
        #[arg(short, long)]
        recursive: bool,
    },

    /// 删除云盘文件或目录（移入回收站）
    #[command(alias = "rm")]
    Delete {
        /// 云盘路径，如 /test/old.txt
        cloud_path: String,
        /// 永久删除（不经过回收站）
        #[arg(long)]
        permanent: bool,
    },

    /// 同步本地目录与云盘目录
    Sync {
        /// 源路径
        src: String,
        /// 目标路径（cloud: 前缀表示云盘路径）
        dest: String,
        /// 删除目标中源没有的文件
        #[arg(long)]
        delete: bool,
    },

    /// 搜索云盘文件
    Search {
        /// 搜索关键词
        keyword: String,
        /// 最大返回条目数（0 = 不限制）
        #[arg(short, long, default_value = "50")]
        limit: usize,
    },
}

#[derive(Subcommand)]
enum ConfigAction {
    /// 显示当前配置
    Show,

    /// 设置 Authorization Token
    Token {
        /// Token 值（Basic base64 格式，从浏览器开发者工具获取）
        value: String,
    },

    /// 设置并行传输数
    Parallel {
        /// 并行数（建议 4~32）
        value: usize,
    },

    /// 设置日志：传入级别（trace/debug/info/warn/error/off）或文件路径
    ///
    /// 示例:
    ///   config log warn           — 设置日志级别为 warn
    ///   config log ./log_tmp      — 日志输出到文件（自动转绝对路径）
    ///   config log off            — 关闭日志
    ///   config log ""             — 清除日志文件设置，恢复终端输出
    Log {
        /// 日志级别或文件路径
        value: String,
    },

    /// 删除配置文件（登出）
    Reset,
}

#[tokio::main]
async fn main() {
    let matches = Cli::command()
        .version(version_string())
        .get_matches();
    let cli = Cli::from_arg_matches(&matches).expect("parse CLI args");

    // config 子命令不需要 auth 和日志
    if let Commands::Config { ref action } = cli.command {
        do_config(action);
        return;
    }

    // 日志初始化（仅非 config 命令时）
    let _log_guard = init_logging();

    // 其余命令：从 config / 环境变量读取 auth + parallel
    let (auth, parallel) = resolve_auth_and_parallel();

    let client = match yun139::Yun139Client::new(&auth) {
        Ok(c) => c,
        Err(e) => {
            eprintln!("❌ {e}");
            std::process::exit(1);
        }
    };

    match cli.command {
        Commands::Config { .. } => unreachable!(),

        Commands::Upload { local_path, cloud_dir } =>
            do_upload(&client, &cloud_dir, &local_path).await,

        Commands::Download { cloud_path, local_path } =>
            do_download(&client, &cloud_path, &local_path, parallel).await,

        Commands::List { cloud_dir } =>
            do_list(&client, &cloud_dir).await,

        Commands::Mkdir { cloud_path, recursive } =>
            do_mkdir(&client, &cloud_path, recursive).await,

        Commands::Delete { cloud_path, permanent } =>
            do_delete(&client, &cloud_path, permanent).await,

        Commands::Sync { src, dest, delete } =>
            do_sync(&client, &src, &dest, delete, parallel).await,

        Commands::Search { keyword, limit } =>
            do_search(&client, &keyword, limit).await,
    }
}

// ── 日志初始化 ──

fn init_logging() -> Option<tracing_appender::non_blocking::WorkerGuard> {
    let (level, file) = yun139::config::Config::load()
        .map(|c| (c.log_level, c.log_file))
        .unwrap_or(("warn".to_string(), None));

    let filter = format!("yun139={level}");

    match file {
        Some(ref path) if !path.is_empty() => {
            let expanded = shellexpand_tilde(path);
            let log_path = std::path::Path::new(&expanded);
            if let Some(parent) = log_path.parent() {
                let _ = std::fs::create_dir_all(parent);
            }
            let file = match std::fs::OpenOptions::new()
                .create(true).append(true).open(&expanded) {
                Ok(f) => f,
                Err(e) => {
                    // 文件打不开，回退到 stderr
                    eprintln!("⚠️  日志文件打开失败 ({e})，输出到终端");
                    tracing_subscriber::fmt()
                        .with_env_filter(&filter)
                        .init();
                    return None;
                }
            };
            let (writer, guard) = tracing_appender::non_blocking(file);
            tracing_subscriber::fmt()
                .with_env_filter(&filter)
                .with_writer(writer)
                .with_ansi(false)
                .init();
            Some(guard)
        }
        _ => {
            tracing_subscriber::fmt()
                .with_env_filter(&filter)
                .init();
            None
        }
    }
}

fn shellexpand_tilde(path: &str) -> String {
    if path.starts_with("~/") {
        if let Some(home) = dirs::home_dir() {
            return format!("{}/{}", home.display(), &path[2..]);
        }
    }
    path.to_string()
}

// ── auth 解析 ──

/// 从 $YUN139_AUTH 或配置文件读取 auth + parallel。
fn resolve_auth_and_parallel() -> (String, usize) {
    let default_p = yun139::config::DEFAULT_PARALLEL;

    // 环境变量优先
    if let Ok(env_auth) = std::env::var("YUN139_AUTH") {
        if !env_auth.is_empty() {
            let p = yun139::config::Config::load().map(|c| c.parallel).unwrap_or(default_p);
            return (env_auth, p);
        }
    }

    match yun139::config::Config::load() {
        Ok(config) => {
            if config.is_expired() {
                eprintln!("⚠️  Token 已过期或即将过期，建议重新设置:");
                eprintln!("   yun139-cli config token <新token>");
            }
            (config.authorization_header(), config.parallel)
        }
        Err(yun139::config::ConfigError::NotFound) => {
            eprintln!("错误: 未找到配置文件");
            eprintln!("  方式 1: yun139-cli config token <token>");
            eprintln!("  方式 2: export YUN139_AUTH=<token>");
            std::process::exit(1);
        }
        Err(e) => {
            eprintln!("❌ 读取配置文件失败: {e}");
            std::process::exit(1);
        }
    }
}

// ── config 子命令 ──

fn do_config(action: &ConfigAction) {
    match action {
        ConfigAction::Show => config_show(),
        ConfigAction::Token { value } => config_token(value),
        ConfigAction::Parallel { value } => config_parallel(*value),
        ConfigAction::Log { value } => config_log(value),
        ConfigAction::Reset => config_reset(),
    }
}

fn config_show() {
    match yun139::config::Config::load() {
        Ok(config) => {
            let status = if config.is_expired() { "⚠️  已过期" } else { "✅ 有效" };
            eprintln!("当前配置:");
            eprintln!("   账号:     {}", config.account);
            eprintln!("   Token:    {}...{}", &config.authorization[..8.min(config.authorization.len())], config.authorization.chars().rev().take(8).collect::<String>().chars().rev().collect::<String>());
            eprintln!("   状态:     {status}");
            eprintln!("   过期:     {}", config.expire_time_display());
            eprintln!("   并行数:   {}", config.parallel);
            eprintln!("   日志级别: {}", config.log_level);
            match &config.log_file {
                Some(f) if !f.is_empty() => eprintln!("   日志文件: {f}"),
                _ => eprintln!("   日志文件: (未设置，输出到终端)"),
            }
            if let Ok(path) = yun139::config::Config::config_path() {
                eprintln!("   配置路径: {}", path.display());
            }
        }
        Err(yun139::config::ConfigError::NotFound) => {
            eprintln!("未配置。请先设置 token:");
            eprintln!("  yun139-cli config token <token>");
        }
        Err(e) => {
            eprintln!("❌ {e}");
            std::process::exit(1);
        }
    }
}

fn config_token(value: &str) {
    match yun139::config::Config::from_token(value) {
        Ok(mut new_config) => {
            if let Ok(old) = yun139::config::Config::load() {
                new_config.parallel = old.parallel;
                new_config.log_level = old.log_level;
                new_config.log_file = old.log_file;
                new_config.personal_cloud_host = old.personal_cloud_host;
            }
            match new_config.save() {
                Ok(path) => {
                    eprintln!("✅ Token 已保存");
                    eprintln!("   账号: {}", new_config.account);
                    eprintln!("   过期: {}", new_config.expire_time_display());
                    eprintln!("   配置: {}", path.display());
                }
                Err(e) => { eprintln!("❌ 保存失败: {e}"); std::process::exit(1); }
            }
        }
        Err(e) => { eprintln!("❌ Token 无效: {e}"); std::process::exit(1); }
    }
}

fn config_parallel(value: usize) {
    update_config(|c| { c.parallel = value; }, &format!("parallel = {value}"));
}

/// 日志设置：值是级别名 → 设置 log_level；是路径 → 设置 log_file（自动转绝对路径）；空串 → 清除 log_file。
fn config_log(value: &str) {
    const LOG_LEVELS: &[&str] = &["trace", "debug", "info", "warn", "error", "off"];

    // 空串或引号包裹的空串 → 清除 log_file
    let trimmed = value.trim().trim_matches('"').trim_matches('\'');
    if trimmed.is_empty() {
        update_config(|c| { c.log_file = None; }, "log_file = (已清除，恢复终端输出)");
    } else if LOG_LEVELS.contains(&trimmed.to_lowercase().as_str()) {
        let level = trimmed.to_lowercase();
        update_config(|c| { c.log_level = level.clone(); }, &format!("log_level = {level}"));
    } else {
        // 当作文件路径处理 → 转绝对路径
        let abs = to_absolute_path(trimmed);
        update_config(|c| { c.log_file = Some(abs.clone()); }, &format!("log_file = {abs}"));
    }
}

/// 将路径转为干净的绝对路径（展开 ~ 和相对路径，清理 ./）。
fn to_absolute_path(path: &str) -> String {
    let expanded = shellexpand_tilde(path);
    let p = std::path::Path::new(&expanded);

    // 先尝试 canonicalize（路径已存在时能解析符号链接和 ./ ../ ）
    if let Ok(abs) = p.canonicalize() {
        return abs.to_string_lossy().to_string();
    }

    // 路径不存在时手动拼接
    let full = if p.is_absolute() {
        p.to_path_buf()
    } else {
        std::env::current_dir().unwrap_or_default().join(p)
    };

    // 用 components() 清理 ./ ../ 等冗余部分
    let mut clean = std::path::PathBuf::new();
    for comp in full.components() {
        match comp {
            std::path::Component::CurDir => {} // 跳过 .
            std::path::Component::ParentDir => { clean.pop(); }
            other => { clean.push(other); }
        }
    }
    clean.to_string_lossy().to_string()
}

/// 加载现有 config → 修改 → 保存。
fn update_config(modify: impl FnOnce(&mut yun139::config::Config), display: &str) {
    match yun139::config::Config::load() {
        Ok(mut config) => {
            modify(&mut config);
            match config.save() {
                Ok(_) => eprintln!("✅ 已更新: {display}"),
                Err(e) => { eprintln!("❌ 保存失败: {e}"); std::process::exit(1); }
            }
        }
        Err(yun139::config::ConfigError::NotFound) => {
            eprintln!("❌ 配置文件不存在，请先设置 token:");
            eprintln!("   yun139-cli config token <token>");
            std::process::exit(1);
        }
        Err(e) => { eprintln!("❌ {e}"); std::process::exit(1); }
    }
}

fn config_reset() {
    match yun139::config::Config::remove() {
        Ok(()) => eprintln!("✅ 配置已重置（文件已删除）"),
        Err(e) => { eprintln!("❌ {e}"); std::process::exit(1); }
    }
}

// ── upload ──

async fn do_upload(client: &yun139::Yun139Client, cloud_dir: &str, local_path: &str) {
    eprintln!("⬆️  上传 {local_path} → 云盘:{cloud_dir}");

    let last = Arc::new(AtomicU64::new(0));
    let last2 = last.clone();

    let result = client.upload(local_path, cloud_dir, move |uploaded, total| {
        let prev = last2.load(Ordering::Relaxed);
        if uploaded - prev >= 1_048_576 || uploaded >= total {
            last2.store(uploaded, Ordering::Relaxed);
            eprint!(
                "\r  {:.1} / {:.1} MB ({:.1}%)",
                uploaded as f64 / 1_048_576.0,
                total as f64 / 1_048_576.0,
                uploaded as f64 / total as f64 * 100.0,
            );
        }
    })
    .await;

    eprintln!();
    match result {
        Ok(file_id) => { eprintln!("✅ 上传完成"); println!("{file_id}"); }
        Err(e) => { eprintln!("❌ 上传失败: {e}"); std::process::exit(1); }
    }
}

// ── download ──

async fn do_download(client: &yun139::Yun139Client, cloud_path: &str, local_path: &str, parallel: usize) {
    eprintln!("⬇️  下载 云盘:{cloud_path} → {local_path}");

    let last = Arc::new(AtomicU64::new(0));
    let last2 = last.clone();

    let result = client.download(cloud_path, local_path, parallel, move |written, total| {
        let prev = last2.load(Ordering::Relaxed);
        if written - prev >= 1_048_576 || total.is_some_and(|t| written >= t) {
            last2.store(written, Ordering::Relaxed);
            if let Some(t) = total {
                eprint!(
                    "\r  {:.1} / {:.1} MB ({:.1}%)",
                    written as f64 / 1_048_576.0,
                    t as f64 / 1_048_576.0,
                    written as f64 / t as f64 * 100.0,
                );
            } else {
                eprint!("\r  {:.1} MB", written as f64 / 1_048_576.0);
            }
        }
    })
    .await;

    eprintln!();
    match result {
        Ok(bytes) => eprintln!("✅ 下载完成: {bytes} bytes"),
        Err(e) => { eprintln!("❌ 下载失败: {e}"); std::process::exit(1); }
    }
}

// ── list ──

async fn do_list(client: &yun139::Yun139Client, cloud_dir: &str) {
    match client.list_all(cloud_dir).await {
        Ok(result) => {
            eprintln!("📂 {} ({} 项)", result.path, result.items.len());
            for item in &result.items {
                let kind = if item.is_folder { "📁" } else { "📄" };
                let size = if item.is_folder { "-".to_string() } else { format_size(item.size) };
                println!("{kind} {:<40} {:>10}  {}", item.name, size, item.updated_at);
            }
        }
        Err(e) => { eprintln!("❌ 列表失败: {e}"); std::process::exit(1); }
    }
}

// ── mkdir ──

async fn do_mkdir(client: &yun139::Yun139Client, cloud_path: &str, recursive: bool) {
    let result = if recursive { client.mkdir_recursive(cloud_path).await } else { client.mkdir(cloud_path).await };
    match result {
        Ok(file_id) => { eprintln!("✅ 目录已创建: {cloud_path}"); println!("{file_id}"); }
        Err(e) => { eprintln!("❌ 创建目录失败: {e}"); std::process::exit(1); }
    }
}

// ── delete ──

async fn do_delete(client: &yun139::Yun139Client, cloud_path: &str, permanent: bool) {
    let result = if permanent {
        eprintln!("🗑️  永久删除 {cloud_path}");
        client.delete(cloud_path).await
    } else {
        eprintln!("🗑️  移入回收站 {cloud_path}");
        client.trash(cloud_path).await
    };
    match result {
        Ok(()) => eprintln!("✅ 删除完成"),
        Err(e) => { eprintln!("❌ 删除失败: {e}"); std::process::exit(1); }
    }
}

// ── sync ──

async fn do_sync(client: &yun139::Yun139Client, src: &str, dest: &str, delete: bool, parallel: usize) {
    let src_is_cloud = src.starts_with("cloud:");
    let dest_is_cloud = dest.starts_with("cloud:");
    let opts = yun139::SyncOptions::default()
        .with_delete(delete)
        .with_concurrency(parallel);

    let result = match (src_is_cloud, dest_is_cloud) {
        (false, true) => {
            let local = std::path::Path::new(src);
            let cloud = dest.strip_prefix("cloud:").unwrap_or(dest);
            eprintln!("🔄 同步 本地:{src} → 云盘:{cloud} (并发={parallel})");
            client.sync_to_cloud_with_options(local, cloud, &opts, |_| {}).await
        }
        (true, false) => {
            let cloud = src.strip_prefix("cloud:").unwrap_or(src);
            let local = std::path::Path::new(dest);
            eprintln!("🔄 同步 云盘:{cloud} → 本地:{dest} (并发={parallel})");
            client.sync_to_local_with_options(cloud, local, &opts, |_| {}).await
        }
        _ => {
            eprintln!("❌ sync 需要一端为本地路径，一端为 cloud: 前缀的云盘路径");
            eprintln!("  示例: yun139-cli sync ./local cloud:/backup");
            eprintln!("  示例: yun139-cli sync cloud:/backup ./local");
            std::process::exit(1);
        }
    };

    match result {
        Ok(summary) => {
            eprintln!();
            eprintln!("✅ 同步完成:");
            eprintln!("   上传: {}  下载: {}  目录: {}  删除: {}  跳过: {}  失败: {}",
                summary.uploaded, summary.downloaded, summary.dirs_created,
                summary.deleted, summary.skipped, summary.failed);
            if summary.failed > 0 { std::process::exit(1); }
        }
        Err(e) => { eprintln!("❌ 同步失败: {e}"); std::process::exit(1); }
    }
}

// ── search ──

async fn do_search(client: &yun139::Yun139Client, keyword: &str, limit: usize) {
    eprintln!("🔍 搜索: {keyword}");
    match client.search(keyword, limit).await {
        Ok(result) => {
            eprintln!("📋 找到 {} 个结果", result.items.len());
            for item in &result.items {
                let kind = if item.is_folder { "📁" } else { "📄" };
                let size = if item.is_folder { "-".to_string() } else { format_size(item.size) };
                println!("{kind} {:<40} {:>10}  {}", item.name, size, item.updated_at);
            }
        }
        Err(e) => { eprintln!("❌ 搜索失败: {e}"); std::process::exit(1); }
    }
}

// ── 工具函数 ──

fn format_size(bytes: i64) -> String {
    const KB: f64 = 1024.0;
    const MB: f64 = KB * 1024.0;
    const GB: f64 = MB * 1024.0;
    let b = bytes as f64;
    if b >= GB { format!("{:.1} GB", b / GB) }
    else if b >= MB { format!("{:.1} MB", b / MB) }
    else if b >= KB { format!("{:.1} KB", b / KB) }
    else { format!("{} B", bytes) }
}
