# yun139

中国移动云盘（139网盘）命令行工具，基于 Rust 构建，支持上传、下载、同步、搜索等操作。

## 功能

| 命令 | 别名 | 说明 |
|------|------|------|
| `upload` | — | 上传本地文件到云盘 |
| `download` | — | 从云盘下载文件到本地 |
| `list` | `ls` | 列出云盘目录内容 |
| `mkdir` | — | 创建云盘目录（支持 `-r` 递归创建） |
| `delete` | `rm` | 删除云盘文件或目录（默认移入回收站） |
| `sync` | — | 双向同步本地目录与云盘目录 |
| `search` | — | 搜索云盘文件 |
| `config` | `cfg` | 管理配置（token、并行数、日志、排除列表） |

## 安装

### 从源码编译

```bash
git clone https://github.com/zjy760401/yun139.git
cd yun139
cargo build --release
# 可执行文件位于 target/release/yun139
```

## 配置

首次使用前需设置 Authorization Token（从浏览器开发者工具抓取 `Authorization` 请求头）：

```bash
yun139 config token <your-token>
```

其他配置项：

```bash
yun139 config show                    # 查看当前配置
yun139 config parallel 8              # 设置并行传输数（建议 4~32）
yun139 config log warn                # 设置日志级别
yun139 config log ./my.log            # 日志输出到文件
yun139 config exclude add "*.tmp"     # 添加上传排除规则
yun139 config reset                   # 删除配置文件（登出）
```

## 使用示例

```bash
# 列出根目录
yun139 ls /

# 上传文件
yun139 upload ./photo.jpg /photos/

# 下载文件
yun139 download /photos/photo.jpg ./photo.jpg

# 创建目录（递归）
yun139 mkdir -r /backup/2024/01

# 删除文件（永久）
yun139 rm /tmp/old.txt --permanent

# 同步本地目录到云盘（仅上传）
yun139 sync ./local-dir cloud:/backup --upload-only

# 搜索文件
yun139 search "报告" -l 20
```

## 许可证

[MIT](LICENSE)
