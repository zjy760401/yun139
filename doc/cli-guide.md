# yun139 CLI 使用指南

## 快速开始

### 1. 安装

```bash
cargo build --release
# 产物位于 target/release/yun139
```

### 2. 配置 Token

从浏览器开发者工具（F12 → Network → 任意 API 请求）复制 `Authorization` 头值：

```bash
yun139 config token "Basic <YOUR_TOKEN>"
```

或使用环境变量（优先级高于配置文件）：

```bash
export YUN139_AUTH="Basic <YOUR_TOKEN>"
```

### 3. 验证

```bash
yun139 list /        # 列出云盘根目录
yun139 config show   # 查看当前配置
```

---

## 命令参考

### upload — 上传文件

```bash
yun139 upload <本地文件> [云盘目录]

# 示例
yun139 upload photo.jpg /photos        # 上传到 /photos/
yun139 upload movie.mp4 /              # 上传到根目录
yun139 upload big.zip                  # 省略目录 = 上传到根目录
```

- 自动选择上传策略（单次 / 分片）
- 支持秒传（SHA256 匹配时跳过实际传输）
- 目标目录不存在时自动创建

### download — 下载文件

```bash
yun139 download <云盘路径> <本地路径>

# 示例
yun139 download /backup/data.zip ./data.zip
```

- 自动探测 Range 支持 → 并行分片下载
- 并行数由 `config parallel` 控制（默认 16）

### list / ls — 列出目录

```bash
yun139 list [云盘目录]

# 示例
yun139 ls                # 列出根目录
yun139 list /photos      # 列出 /photos
```

输出格式：`类型 文件名 大小 修改时间`

### mkdir — 创建目录

```bash
yun139 mkdir <云盘路径> [-r]

# 示例
yun139 mkdir /backup
yun139 mkdir /a/b/c/d -r    # 递归创建
```

### delete / rm — 删除

```bash
yun139 delete <云盘路径> [--permanent]

# 示例
yun139 rm /old/file.txt           # 移入回收站
yun139 delete /old --permanent    # 永久删除
```

### sync — 双向同步

```bash
yun139 sync <源> <目标> [选项]

# 上传同步：本地 → 云盘
yun139 sync ./local cloud:/backup

# 下载同步：云盘 → 本地
yun139 sync cloud:/backup ./local

# 带删除（目标中源没有的文件将被删除）
yun139 sync ./local cloud:/backup --delete

# 仅上传（跳过下载）
yun139 sync ./local cloud:/backup --upload-only

# 仅下载（跳过上传）
yun139 sync cloud:/data ./data --download-only
```

进度展示：
```
⠹ scan /photos/2024
sync [████████████░░░░░░░░] 24/50 (48%) ↑18 ↓6
     ↑ photo_001.jpg [━━━━━━━━╸─────] 3.2MB/8.0MB 2.1MB/s
     ↓ video.mp4     [━━━━━━━━━━━━╸] 45MB/50MB 5.3MB/s
```

### search — 搜索

```bash
yun139 search <关键词> [-l 数量]

# 示例
yun139 search "photo" -l 20     # 最多 20 条
yun139 search "2024"            # 默认 50 条
```

### config — 配置管理

```bash
# 查看配置
yun139 config show

# Token 管理
yun139 config token <value>
yun139 config reset              # 删除配置（登出）

# 并行数
yun139 config parallel 8

# 日志
yun139 config log info           # 设置级别
yun139 config log ./logs         # 输出到文件
yun139 config log ""             # 清除文件设置

# 排除列表
yun139 config exclude            # 查看列表
yun139 config exclude add "*.tmp"
yun139 config exclude rm ".DS_Store"
yun139 config exclude reset      # 恢复默认
```

---

## 环境变量

| 变量 | 说明 |
|------|------|
| `YUN139_AUTH` | Authorization token（优先于配置文件） |

---

## 退出码

| 码 | 含义 |
|----|------|
| 0 | 成功 |
| 1 | 失败（网络/API/IO/配置错误，或 sync 有失败文件） |
