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
        eprintln!("用法: {} <authorization> <cloud_dir> <local_path>", args[0]);
        std::process::exit(1);
    }

    let last_printed = Arc::new(AtomicU64::new(0));

    let result = yun139::upload(
        &args[1],
        &args[2],
        &args[3],
        |uploaded, total| {
            let last = last_printed.load(Ordering::Relaxed);
            if uploaded - last >= 1_048_576 || uploaded >= total {
                last_printed.store(uploaded, Ordering::Relaxed);
                let pct = uploaded as f64 / total as f64 * 100.0;
                let mb = uploaded as f64 / 1_048_576.0;
                let total_mb = total as f64 / 1_048_576.0;
                eprint!("\r  {:.1} / {:.1} MB ({:.1}%)", mb, total_mb, pct);
            }
        },
    )
    .await;

    eprintln!();
    match result {
        Ok(file_id) => println!("✅ 上传完成: fileId={file_id}"),
        Err(e) => {
            eprintln!("❌ 上传失败: {e}");
            std::process::exit(1);
        }
    }
}
