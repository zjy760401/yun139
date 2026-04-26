/// 构建脚本：从 git 获取 commit count，注入环境变量。
fn main() {
    // 获取 git commit count
    let count = std::process::Command::new("git")
        .args(["rev-list", "--count", "HEAD"])
        .output()
        .ok()
        .and_then(|o| String::from_utf8(o.stdout).ok())
        .map(|s| s.trim().to_string())
        .unwrap_or_else(|| "0".to_string());

    println!("cargo:rustc-env=GIT_COMMIT_COUNT={count}");

    // git 变更时重新运行
    println!("cargo:rerun-if-changed=.git/HEAD");
    println!("cargo:rerun-if-changed=.git/refs/");
}
