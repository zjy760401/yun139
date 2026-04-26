//! mcloud-sign 签名算法。
//!
//! Web 端 API 要求每个请求携带 `mcloud-sign` 请求头，格式:
//!   `<timestamp>,<rand>,<sign>`
//!
//! 签名计算流程（来自 cloud139 参考实现）:
//!   1. 对 JSON body 做 encodeURIComponent
//!   2. 将编码后的字符逐字符排序
//!   3. base64 编码排序结果
//!   4. md5(base64结果) => hash1
//!   5. md5("<timestamp>:<rand>") => hash2
//!   6. md5(hash1 + hash2) => 最终签名（大写）

use digest::Digest;

/// 生成指定长度的随机字母数字串
pub fn rand_str(len: usize) -> String {
    use rand::Rng;
    const CHARSET: &[u8] = b"abcdefghijklmnopqrstuvwxyzABCDEFGHIJKLMNOPQRSTUVWXYZ0123456789";
    let mut rng = rand::thread_rng();
    (0..len)
        .map(|_| CHARSET[rng.gen_range(0..CHARSET.len())] as char)
        .collect()
}

fn md5_hex(data: &str) -> String {
    let mut hasher = md5::Md5::new();
    hasher.update(data.as_bytes());
    hex::encode(hasher.finalize())
}

/// JS encodeURIComponent 的实现（与 JavaScript 行为一致）。
/// 不编码: A-Z a-z 0-9 - _ . ! ~ * ' ( )
fn encode_uri_component(s: &str) -> String {
    let mut out = String::new();
    for c in s.chars() {
        if c.is_ascii_alphanumeric()
            || c == '-' || c == '_' || c == '.' || c == '~'
            || c == '!' || c == '*' || c == '\'' || c == '(' || c == ')'
        {
            out.push(c);
        } else {
            for b in c.to_string().as_bytes() {
                out.push_str(&format!("%{:02X}", b));
            }
        }
    }
    out
}

/// 计算 mcloud-sign 的签名部分
pub fn calc_sign(body: &str, ts: &str, rand: &str) -> String {
    let encoded = encode_uri_component(body);
    let mut chars: Vec<char> = encoded.chars().collect();
    chars.sort();
    let sorted: String = chars.into_iter().collect();

    let body_b64 = base64::Engine::encode(
        &base64::engine::general_purpose::STANDARD,
        &sorted,
    );

    let h1 = md5_hex(&body_b64);
    let h2 = md5_hex(&format!("{}:{}", ts, rand));
    md5_hex(&format!("{}{}", h1, h2)).to_uppercase()
}

/// 生成完整的 mcloud-sign 头值: "ts,rand,sign"
pub fn make_mcloud_sign(body: &str) -> String {
    let ts = chrono::Local::now().format("%Y-%m-%d %H:%M:%S").to_string();
    let r = rand_str(16);
    let sign = calc_sign(body, &ts, &r);
    format!("{},{},{}", ts, r, sign)
}
