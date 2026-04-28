# yun139 项目结构

```
yun139/
├── Cargo.toml                 # 项目配置（lib + bin 双 target）
├── Cargo.lock                 # 依赖锁定
├── build.rs                   # 构建脚本（注入 git commit count）
├── .gitignore
│
├── src/
│   ├── main.rs                # CLI 入口（clap 解析 + 命令分发）    713 行
│   ├── lib.rs                 # SDK 门面（pub 导出 + 便捷函数）     129 行
│   ├── client.rs              # 核心客户端（HTTP + 路由 + 路径解析） 397 行
│   ├── api.rs                 # API 请求/响应类型定义               265 行
│   ├── sign.rs                # mcloud-sign 签名算法                 74 行
│   ├── error.rs               # 统一错误枚举                         30 行
│   ├── config.rs              # 配置文件管理                        194 行
│   │
│   └── commands/
│       ├── mod.rs             # 模块声明                              9 行
│       ├── download.rs        # 下载（单流 + 并行分片）             262 行
│       ├── upload.rs          # 上传（单次 + 分片 + 秒传）          327 行
│       ├── list.rs            # 目录列表（全量 + 分页）             147 行
│       ├── mkdir.rs           # 创建目录（单层 + 递归）              86 行
│       ├── delete.rs          # 删除（回收站 + 永久）                60 行
│       ├── search.rs          # 文件搜索                             78 行
│       └── sync.rs            # 双向同步（流式并行）                858 行
│
├── examples/
│   ├── basic_download.rs      # 下载示例
│   ├── basic_upload.rs        # 上传示例
│   └── list_root.rs           # 列表示例
│
├── tests/
│   ├── integration_all_commands.rs   # SDK 全命令集成测试
│   └── upload_download_roundtrip.rs  # CLI 200MB roundtrip E2E 测试
│
├── doc/                       # 设计文档
│   ├── architecture.md        # 架构设计文档
│   ├── features.md            # 功能详细文档
│   ├── api-protocol.md        # API 协议文档
│   ├── cli-guide.md           # CLI 使用指南
│   ├── sdk-guide.md           # SDK 开发指南
│   ├── testing.md             # 测试文档
│   └── project-structure.md   # 本文件
│
└── log_tmp/                   # 日志临时目录（.gitignore）
```

## 代码统计

| 模块 | 行数 | 占比 |
|------|------|------|
| sync.rs | 858 | 24% |
| main.rs | 713 | 20% |
| client.rs | 397 | 11% |
| upload.rs | 327 | 9% |
| api.rs | 265 | 7% |
| download.rs | 262 | 7% |
| config.rs | 194 | 5% |
| list.rs | 147 | 4% |
| lib.rs | 129 | 4% |
| mkdir.rs | 86 | 2% |
| search.rs | 78 | 2% |
| sign.rs | 74 | 2% |
| delete.rs | 60 | 2% |
| error.rs | 30 | 1% |
| commands/mod.rs | 9 | <1% |
| **合计** | **3,629** | **100%** |
