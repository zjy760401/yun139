use std::env;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;

#[tokio::main]
async fn main() {
    tracing_subscriber::fmt()
        .with_env_filter("yun139=info")
        .init();

    let args: Vec<String> = env::args().collect();
    if args.len() < 4 {
        eprintln!("用法: {} <authorization> <cloud_path> <local_path> [parallel]", args[0]);
        eprintln!("  parallel: 并发数，默认 4");
        std::process::exit(1);
    }

    let parallel: usize = args.get(4).and_then(|s| s.parse().ok()).unwrap_or(4);
    let last_printed = Arc::new(AtomicU64::new(0));

    let result = yun139::download(
        &args[1],
        &args[2],
        &args[3],
        parallel,
        move |written, total| {
            let last = last_printed.load(Ordering::Relaxed);
            if written - last >= 1_048_576 || total.is_some_and(|t| written >= t) {
                last_printed.store(written, Ordering::Relaxed);
                if let Some(t) = total {
                    let pct = written as f64 / t as f64 * 100.0;
                    let mb = written as f64 / 1_048_576.0;
                    let total_mb = t as f64 / 1_048_576.0;
                    eprint!("\r  {:.1} / {:.1} MB ({:.1}%)", mb, total_mb, pct);
                } else {
                    eprint!("\r  {:.1} MB", written as f64 / 1_048_576.0);
                }
            }
        },
    )
    .await;

    eprintln!();
    match result {
        Ok(bytes) => println!("✅ 下载完成: {} 字节 → {} (并发={})", bytes, args[3], parallel),
        Err(e) => {
            eprintln!("❌ 下载失败: {e}");
            std::process::exit(1);
        }
    }
}
