use std::env;

#[tokio::main]
async fn main() {
    tracing_subscriber::fmt()
        .with_env_filter("yun139=debug")
        .init();

    let args: Vec<String> = env::args().collect();
    if args.len() < 2 {
        eprintln!("用法: {} <authorization>", args[0]);
        std::process::exit(1);
    }

    let client = yun139::Yun139Client::new(&args[1]).expect("init failed");
    match client.list_files("/", "").await {
        Ok(resp) => {
            println!("success={}, code={:?}", resp.success, resp.code);
            if let Some(data) = resp.data {
                println!("共 {} 个项目:", data.items.len());
                for item in &data.items {
                    println!("  [{:6}] {:30} id={} size={}",
                        item.file_type.as_deref().unwrap_or("?"),
                        item.name.as_deref().unwrap_or("?"),
                        item.file_id.as_deref().unwrap_or("?"),
                        item.size.unwrap_or(0),
                    );
                }
            }
        }
        Err(e) => eprintln!("❌ {e}"),
    }
}
