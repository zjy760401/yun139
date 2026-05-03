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

## sync 并发策略

`sync` 采用**流水线 + 任务池**模型，充分利用网络与磁盘 IO 的可并行性，同时避免资源竞争。

### 整体架构

```
┌─────────────────────────────────────────────────────────┐
│  主协程（streaming_sync）                                │
│                                                          │
│  spawn_counted(walk_root)  ──► 任务池                   │
│                                    │                     │
│  等待 active_tasks == 0  ◄─────────┘                    │
│                                                          │
│  串行执行 pending_deletes（深度优先排序）                │
└─────────────────────────────────────────────────────────┘
```

### Stage 1 — Walker（BFS 目录扫描）

每个目录由一个独立协程处理，整体以 **广度优先（BFS）** 顺序展开：

1. 获取 `global_sem` permit → 并发调用 `list_cloud` + 读本地目录
2. 释放 permit（扫描完成后立刻归还，不阻塞传输）
3. 对比结果，将任务分发到 Stage 2
4. 对每个子目录，通过 `spawn_counted` 再次递归，形成 BFS 展开树

Walker 和 Worker 共享同一个 `global_sem`，因此扫描 API 请求与文件传输天然互相竞争配额，不会因大量目录扫描而挤占传输带宽。

### Stage 2 — Workers（并发传输）

对比后的每个文件任务通过 `spawn_counted` 异步分发，共三类：

| 任务类型 | 行为 |
|----------|------|
| **直接上传 / 下载** | 持有 `global_sem` → 传输 → 释放 |
| **SHA256 校验后传输** | 先持有 `hash_sem` 计算本地哈希 → 释放 `hash_sem` → 若需传输则持有 `global_sem` → 传输 |
| **跳过** | 无需任何信号量，直接计数 |

### 信号量设计

```
global_sem  (容量 = --parallel，默认 8)
  ├── 目录扫描（list API）短暂持有
  ├── 文件上传（持有全程）
  └── 文件下载（持有全程）

hash_sem    (容量 = min(parallel, 4))
  └── SHA256 磁盘读取（独立于 global_sem）
      释放后才竞争 global_sem 发起传输
```

`hash_sem` 独立存在的原因：SHA256 是纯磁盘 IO，与网络传输不争带宽，但多个文件同时读取会造成寻道竞争（外置盘尤为明显），因此单独限流为 4 路。

### 任务计数与退出

使用 `AtomicU32 active_tasks` + `Notify all_done` 替代 `JoinSet`：

```
spawn_counted(fut):
  active_tasks += 1   // 先递增，防止任务完成前计数归零误判
  tokio::spawn(fut)
    └── 任务结束时: active_tasks -= 1
                      if active_tasks == 0 → all_done.notify()

主协程:
  while active_tasks > 0 { all_done.notified().await }
```

先递增再 spawn 是关键：若先 spawn 后递增，任务可能在递增前已完成，导致 `active_tasks` 提前归零触发假退出。

### 删除时序

删除操作**不在扫描阶段执行**，而是收集到 `pending_deletes` 中，待所有传输完成后统一串行执行，并按**路径深度降序排序**（子目录先于父目录删除），避免删除父目录时子目录尚未处理。

### 文件对比规则（sync 决策表）

| 情况 | 普通 sync | `--force-local` | `--force-remote` |
|------|-----------|-----------------|------------------|
| 云端不存在 | 上传 | 上传 | 本地删除 |
| 本地不存在 | 下载 | 云端删除 | 下载 |
| size 不同，本地更新 | 上传 | 上传 | 下载 |
| size 不同，云端更新 | 跳过 | 上传 | 下载 |
| size + mtime 相同 | 跳过 | 跳过 | 跳过 |
| size 相同，mtime 不同 | SHA256 对比，按 mtime 方向传输 | SHA256 对比，不一致则上传 | SHA256 对比，不一致则下载 |
| 目标侧多余文件 | 保留（需 `--delete`） | 删除云端 | 删除本地 |

> SHA256 只在 size 相同但 mtime 不同时触发，避免对大量文件做全量哈希计算。

## 许可证

[MIT](LICENSE)
