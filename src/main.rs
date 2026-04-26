use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;

use clap::{Parser, Subcommand};

/// 中国移动云盘 (139网盘) CLI
#[derive(Parser)]
#[command(name = "yun139-cli", version, about = "139 云盘命令行工具")]
struct Cli {
    /// Authorization 令牌 (Basic base64 格式)，也可通过 YUN139_AUTH 环境变量设置。
    /// 未指定时自动从 ~/.config/yun139/config.toml 读取。
    #[arg(short, long, env = "YUN139_AUTH", global = true, hide_env_values = true)]
    auth: Option<String>,

    /// 并行数（下载分片并发、sync 文件并发等共用，默认从配置读取或 16）
    #[arg(short, long, global = true)]
    parallel: Option<usize>,

    /// 日志级别 (如 yun139=debug)
    #[arg(long, default_value = "yun139=info", global = true)]
    log: String,

    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// 登录：保存 token 到配置文件，后续命令无需再传 --auth
    Login {
        /// Authorization Token (从浏览器开发者工具获取)
        token: Option<String>,
    },

    /// 登出：删除保存的配置文件
    Logout,

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

#[tokio::main]
async fn main() {
    let cli = Cli::parse();

    tracing_subscriber::fmt()
        .with_env_filter(&cli.log)
        .init();

    // login / logout 不需要 auth
    match &cli.command {
        Commands::Login { token } => {
            do_login(token.as_deref());
            return;
        }
        Commands::Logout => {
            do_logout();
            return;
        }
        _ => {}
    }

    // 其余命令：解析 auth (--auth > $YUN139_AUTH > config.toml)
    let (auth, config_parallel) = resolve_auth_and_parallel(cli.auth.as_deref());
    let parallel = cli.parallel.unwrap_or(config_parallel);

    let client = match yun139::Yun139Client::new(&auth) {
        Ok(c) => c,
        Err(e) => {
            eprintln!("❌ {e}");
            std::process::exit(1);
        }
    };

    match cli.command {
        Commands::Login { .. } | Commands::Logout => unreachable!(),

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

// ── auth 解析 ──

/// 解析 auth 和 parallel。
/// 返回 (authorization, config_parallel)。
/// parallel 优先级: -p 命令行 > config.parallel > DEFAULT_PARALLEL
fn resolve_auth_and_parallel(cli_auth: Option<&str>) -> (String, usize) {
    let default_p = yun139::config::DEFAULT_PARALLEL;

    if let Some(auth) = cli_auth {
        // 有 --auth 时尝试读 config 的 parallel，读不到用默认
        let p = yun139::config::Config::load().map(|c| c.parallel).unwrap_or(default_p);
        return (auth.to_string(), p);
    }

    // 尝试从配置文件加载
    match yun139::config::Config::load() {
        Ok(config) => {
            if config.is_expired() {
                eprintln!("⚠️  Token 已过期或即将过期，建议重新 `yun139-cli login`");
            }
            let p = config.parallel;
            (config.authorization_header(), p)
        }
        Err(yun139::config::ConfigError::NotFound) => {
            eprintln!("错误: 未提供令牌，且未找到保存的登录信息");
            eprintln!("  方式 1: yun139-cli login <token>     ← 保存后自动使用");
            eprintln!("  方式 2: yun139-cli -a <token> <cmd>  ← 单次使用");
            eprintln!("  方式 3: export YUN139_AUTH=<token>    ← 环境变量");
            std::process::exit(1);
        }
        Err(e) => {
            eprintln!("❌ 读取配置文件失败: {e}");
            std::process::exit(1);
        }
    }
}

// ── login ──

fn do_login(token: Option<&str>) {
    match token {
        Some(t) => {
            // 保存 token
            match yun139::config::Config::from_token(t) {
                Ok(config) => {
                    match config.save() {
                        Ok(path) => {
                            eprintln!("✅ 登录成功!");
                            eprintln!("   账号: {}", config.account);
                            eprintln!("   过期: {}", config.expire_time_display());
                            eprintln!("   配置: {}", path.display());
                        }
                        Err(e) => {
                            eprintln!("❌ 保存配置失败: {e}");
                            std::process::exit(1);
                        }
                    }
                }
                Err(e) => {
                    eprintln!("❌ Token 无效: {e}");
                    std::process::exit(1);
                }
            }
        }
        None => {
            // 无参数：显示当前登录状态
            match yun139::config::Config::load() {
                Ok(config) => {
                    let status = if config.is_expired() { "⚠️  已过期" } else { "✅ 有效" };
                    eprintln!("当前登录信息:");
                    eprintln!("   账号: {}", config.account);
                    eprintln!("   状态: {status}");
                    eprintln!("   过期: {}", config.expire_time_display());
                    if let Ok(path) = yun139::config::Config::config_path() {
                        eprintln!("   配置: {}", path.display());
                    }
                }
                Err(yun139::config::ConfigError::NotFound) => {
                    eprintln!("未登录。用法:");
                    eprintln!("  yun139-cli login <token>");
                    eprintln!("  token 从浏览器开发者工具 → Network → 请求头 → Authorization 获取");
                }
                Err(e) => {
                    eprintln!("❌ 读取配置失败: {e}");
                    std::process::exit(1);
                }
            }
        }
    }
}

// ── logout ──

fn do_logout() {
    match yun139::config::Config::remove() {
        Ok(()) => eprintln!("✅ 已登出，配置文件已删除"),
        Err(e) => {
            eprintln!("❌ 删除配置失败: {e}");
            std::process::exit(1);
        }
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
        Ok(file_id) => {
            eprintln!("✅ 上传完成");
            println!("{file_id}");
        }
        Err(e) => {
            eprintln!("❌ 上传失败: {e}");
            std::process::exit(1);
        }
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
        Err(e) => {
            eprintln!("❌ 下载失败: {e}");
            std::process::exit(1);
        }
    }
}

// ── list ──

async fn do_list(client: &yun139::Yun139Client, cloud_dir: &str) {
    match client.list_all(cloud_dir).await {
        Ok(result) => {
            eprintln!("📂 {} ({} 项)", result.path, result.items.len());
            for item in &result.items {
                let kind = if item.is_folder { "📁" } else { "📄" };
                let size = if item.is_folder {
                    "-".to_string()
                } else {
                    format_size(item.size)
                };
                println!("{kind} {:<40} {:>10}  {}", item.name, size, item.updated_at);
            }
        }
        Err(e) => {
            eprintln!("❌ 列表失败: {e}");
            std::process::exit(1);
        }
    }
}

// ── mkdir ──

async fn do_mkdir(client: &yun139::Yun139Client, cloud_path: &str, recursive: bool) {
    let result = if recursive {
        client.mkdir_recursive(cloud_path).await
    } else {
        client.mkdir(cloud_path).await
    };

    match result {
        Ok(file_id) => {
            eprintln!("✅ 目录已创建: {cloud_path}");
            println!("{file_id}");
        }
        Err(e) => {
            eprintln!("❌ 创建目录失败: {e}");
            std::process::exit(1);
        }
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
        Err(e) => {
            eprintln!("❌ 删除失败: {e}");
            std::process::exit(1);
        }
    }
}

// ── sync ──

async fn do_sync(client: &yun139::Yun139Client, src: &str, dest: &str, delete: bool, concurrency: usize) {
    let src_is_cloud = src.starts_with("cloud:");
    let dest_is_cloud = dest.starts_with("cloud:");
    let opts = yun139::SyncOptions::default()
        .with_delete(delete)
        .with_concurrency(concurrency);

    let result = match (src_is_cloud, dest_is_cloud) {
        (false, true) => {
            let local = std::path::Path::new(src);
            let cloud = dest.strip_prefix("cloud:").unwrap_or(dest);
            eprintln!("🔄 同步 本地:{src} → 云盘:{cloud} (并发={concurrency})");
            client.sync_to_cloud_with_options(local, cloud, &opts, |msg| eprintln!("  {msg}")).await
        }
        (true, false) => {
            let cloud = src.strip_prefix("cloud:").unwrap_or(src);
            let local = std::path::Path::new(dest);
            eprintln!("🔄 同步 云盘:{cloud} → 本地:{dest} (并发={concurrency})");
            client.sync_to_local_with_options(cloud, local, &opts, |msg| eprintln!("  {msg}")).await
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
            if summary.failed > 0 {
                std::process::exit(1);
            }
        }
        Err(e) => {
            eprintln!("❌ 同步失败: {e}");
            std::process::exit(1);
        }
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
                let size = if item.is_folder {
                    "-".to_string()
                } else {
                    format_size(item.size)
                };
                println!("{kind} {:<40} {:>10}  {}", item.name, size, item.updated_at);
            }
        }
        Err(e) => {
            eprintln!("❌ 搜索失败: {e}");
            std::process::exit(1);
        }
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
