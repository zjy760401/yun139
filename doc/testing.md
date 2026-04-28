# yun139 测试文档

## 1. 测试概览

| 测试 | 类型 | 文件 | 说明 |
|------|------|------|------|
| `sdk_all_commands` | 集成 | `tests/integration_all_commands.rs` | SDK API 全命令覆盖 |
| `upload_download_roundtrip_via_cli` | E2E | `tests/upload_download_roundtrip.rs` | CLI 200MB 端到端 roundtrip |
| `examples/basic_download` | 示例 | `examples/basic_download.rs` | 下载示例 |
| `examples/basic_upload` | 示例 | `examples/basic_upload.rs` | 上传示例 |
| `examples/list_root` | 示例 | `examples/list_root.rs` | 列表示例 |

## 2. 运行方式

### 前置条件

所有测试需要有效的 `YUN139_AUTH` 环境变量（真实 token），未设置时测试自动跳过。

### 集成测试

```bash
# 全命令 SDK 测试（需要网络）
YUN139_AUTH="Basic cGM6MTM5..." cargo test --test integration_all_commands -- --nocapture

# 200MB CLI roundtrip 测试（需要先 cargo build）
cargo build
YUN139_AUTH="Basic cGM6MTM5..." cargo test --test upload_download_roundtrip -- --nocapture
```

### 示例

```bash
cargo run --example basic_download -- "Basic xxx" /path/to/file ./output 4
cargo run --example basic_upload -- "Basic xxx" /cloud_dir ./local_file
cargo run --example list_root -- "Basic xxx"
```

## 3. SDK 全命令测试详情

`integration_all_commands.rs` 在云盘创建临时目录 `/yun139_sdk_test_<timestamp>`，依次测试：

1. **list** — 列出根目录，验证 API 连通性
2. **mkdir** — 创建单层 + 递归多层目录
3. **upload (小文件)** — 5MB 随机文件，走单次 PUT 路径
4. **upload (大文件)** — 15MB 随机文件，走分片上传路径
5. **list** — 验证上传结果（文件名、大小）
6. **search** — 用时间戳搜索，验证 API 成功（搜索有索引延迟）
7. **download (单流)** — 单流下载小文件 + SHA256 校验
8. **download (并行)** — 并行下载大文件 + SHA256 校验
9. **trash** — 大文件移入回收站，验证列表消失
10. **delete** — 永久删除小文件
11. **list** — 验证无文件残留
12. **error cases** — 测试 PathNotFound、IsDirectory 等错误路径
13. **cleanup** — 删除测试目录

### 清理保障

- `CleanupGuard` 结构体在 Drop 时清理本地临时文件
- 云盘清理在测试流程中显式执行（异步操作无法在 Drop 中执行）

### 随机文件生成

使用 LCG 伪随机数生成器（非 cryptographic），确保：
- 每次运行内容不同 → 不会命中秒传
- 可重复验证 SHA256 完整性

## 4. CLI Roundtrip 测试详情

`upload_download_roundtrip.rs` 测试端到端 CLI 工作流：

1. 生成 200MB 随机文件 + 计算 SHA256
2. 通过 CLI `yun139 upload` 上传
3. 通过 CLI `yun139 download` 下载
4. SHA256 校验下载文件 == 原始文件
5. 清理

> **注意**: 此测试调用编译好的 `yun139` 二进制，需要先 `cargo build`。

## 5. 测试不覆盖的部分

- sync 命令（需要复杂的目录结构 + 双向同步场景）
- config 子命令（文件系统副作用）
- Token 过期场景
- 网络异常 / 重试路径（需要 mock server）
- 并发竞争条件
