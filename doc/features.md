# yun139 功能详细文档

## 1. 上传 (upload)

### 1.1 入口

```
CLI:  yun139 upload <local_path> [cloud_dir]
SDK:  client.upload_file(&path, cloud_dir, on_progress)
```

### 1.2 策略选择

| 文件大小 | 策略 | 分片大小 |
|----------|------|----------|
| ≤ 10 MB | 单次 PUT | 整文件 |
| > 10 MB, ≤ 30 GB | 并行分片 PUT | 100 MB/片 |
| > 30 GB | 并行分片 PUT | 512 MB/片 |

### 1.3 上传流程

```
1. 读取文件 metadata (size, name)
2. 计算整文件 SHA256 (spawn_blocking, 2MB 缓冲)
3. 确保云盘目标目录存在 (ensure_dir → mkdir -p)
4. POST /file/create
   ├─ 携带 contentHash, size, partInfos
   ├─ 若 rapidUpload=true → 秒传成功，立即返回
   └─ 返回 fileId, uploadId, partInfos
5. POST /file/getUploadUrl (每 100 片一批)
   └─ 返回每片的 OSS PUT URL
6. PUT 分片到 OSS
   ├─ 小文件: 整块读入内存 → 单次 PUT
   └─ 大文件: 流式读取 (256KB chunks) → 2 并发上传
7. POST /file/complete
   └─ 携带 contentHash, uploadId, fileId
```

### 1.4 重试机制

- 每个分片 PUT 最多重试 5 次
- 指数退避：`2^attempt` 秒（上限 16 秒）
- 失败时完整重发该分片

### 1.5 秒传检测

服务端通过 SHA256 判断文件是否已存在：
- `rapid_upload == true` → 文件已存在，无需上传
- `upload_id` 为空 → 视为秒传
- `part_infos` 为空 → 视为秒传

### 1.6 Content-Type 推断

根据文件扩展名自动推断，支持：
mp4, mov, avi, mkv, mp3, pdf, zip, gz/tgz, tar, png, jpg/jpeg, gif, svg, txt, html/htm, json, xml
未识别扩展名 → `application/octet-stream`

---

## 2. 下载 (download)

### 2.1 入口

```
CLI:  yun139 download <cloud_path> <local_path>
SDK:  client.download(cloud_path, local_path, parallel, on_progress)
```

### 2.2 流程

```
1. resolve_path(cloud_path) → 解析为 fileId
2. get_download_url(fileId) → 获取 CDN URL
3. 根据 parallel 参数选择模式:
   ├─ parallel ≤ 1 → 单流下载
   └─ parallel > 1 → 探测 Range 支持 → 分片并行下载
```

### 2.3 并行下载策略

```
1. 发送 Range: bytes=0-0 探测请求
2. 若 206 Partial Content → 从 Content-Range 解析文件总大小
3. 若不支持 Range 或文件 < 8MB → 回退到单流
4. 切分为 8MB 分片 → Semaphore 控制并发数
5. 每片独立 Range GET → seek 写入文件对应偏移
6. 全局 AtomicU64 累加进度 → 实时回调
```

### 2.4 重试机制

- 单流模式：整个下载最多重试 3 次
- 并行模式：每个分片独立重试 3 次
- 退避策略：`500ms × attempt`
- 分片失败时回退全局进度计数器，避免进度虚报

### 2.5 文件写入

- 并行模式预先 `set_len(file_size)` 分配空间
- 各分片通过 `AsyncSeekExt::seek` 写入各自偏移
- 下载完成校验：`total_written == file_size`

---

## 3. 目录列表 (list)

### 3.1 入口

```
CLI:  yun139 list [cloud_dir]      # 别名: yun139 ls
SDK:  client.list_all(cloud_dir)   # 全量遍历
      client.list_page(dir, cursor, page_size)  # 分页
```

### 3.2 实现

- `list_all`: 自动分页遍历，每页 100 条，直到 `next_page_cursor` 为空
- `list_page`: 单页查询，返回 `(items, next_cursor)`
- 路径解析：非根目录先 `resolve_path` → 获取 `parent_file_id`

### 3.3 返回数据

```rust
ListItem {
    file_id: String,
    name: String,
    size: i64,
    is_folder: bool,
    updated_at: String,       // RFC3339 时间戳
    content_hash: String,     // SHA256（文件夹为空）
}
```

---

## 4. 创建目录 (mkdir)

### 4.1 入口

```
CLI:  yun139 mkdir <cloud_path> [-r]
SDK:  client.mkdir(path)              # 单层
      client.mkdir_recursive(path)    # 递归
```

### 4.2 实现

- **单层 (`mkdir`)**: 拆分路径为 `(parent_dir, name)`，解析父目录 fileId → POST /file/create
- **递归 (`mkdir_recursive`)**: 委托给 `ensure_dir()`，逐级检查并创建
- `fileRenameMode: "refuse"` — 同名目录已存在时拒绝而非重命名

---

## 5. 删除 (delete)

### 5.1 入口

```
CLI:  yun139 delete <cloud_path> [--permanent]    # 别名: yun139 rm
SDK:  client.trash(path)     # 移入回收站
      client.delete(path)   # 永久删除
```

### 5.2 实现

- 先 `resolve_path` 获取 fileId（禁止操作根目录）
- **回收站**: POST `/recyclebin/batchTrash`
- **永久删除**: POST `/file/batchDelete`
- 均支持批量（当前实现为单条调用）

---

## 6. 搜索 (search)

### 6.1 入口

```
CLI:  yun139 search <keyword> [-l limit]
SDK:  client.search(keyword, limit)
```

### 6.2 实现

- POST `/file/search`，从根目录递归搜索
- 分页遍历（每页 100 条），`limit > 0` 时提前截断
- 返回与 list 相同的 `ListItem` 结构

---

## 7. 同步 (sync)

### 7.1 入口

```
CLI:  yun139 sync <src> <dest> [--delete] [--upload-only] [--download-only]
      src/dest 格式: 本地路径 或 cloud:/cloud/path
SDK:  client.sync_to_cloud(local, cloud, delete, on_progress)
      client.sync_to_local(cloud, local, delete, on_progress)
      client.sync_to_cloud_with_options(local, cloud, &opts, on_progress)
      client.sync_to_local_with_options(cloud, local, &opts, on_progress)
```

### 7.2 并行模型

```
global_sem(P)  ← 总并行度上限（P = config.parallel）
scan_sem(2)    ← 扫描子限额，最多 2 个目录同时扫描

scan  = 生产者（发现差异 → 推入传输任务）
transfer = 消费者（执行实际上传/下载）

┌─ scan_dir ─────────────────────────────────┐
│  acquire scan_sem                           │
│  acquire global_sem  ← 背压点              │
│     若 transfer 占满 global permit,         │
│     scan 自然等待（消费完再生产）             │
│                                             │
│  并行获取: 本地目录列表 + 云盘目录列表       │
│  Phase A: 对比差异，收集待传输列表           │
│  Phase B: 更新进度条                        │
│  Phase C: 创建目录 + spawn 子目录 scan      │
│  Phase D: 排队提交传输到 JoinSet            │
│  Phase E: 收集待删除项                      │
│  Phase F: 等待子目录 scan 完成              │
└─────────────────────────────────────────────┘

┌─ transfer (JoinSet) ───────────────────────┐
│  acquire global_sem                         │
│  执行 upload_file / download_parallel       │
└─────────────────────────────────────────────┘
```

### 7.3 文件差异判断

**LocalToCloud 方向：**

| 条件 | 行为 |
|------|------|
| 云盘无此文件 | 上传 |
| size 不同 + 本地更新 | 上传 |
| size 不同 + 云盘更新 | 跳过 |
| size 相同 + hash 不同 | 上传 |
| size 相同 + hash 相同 | 跳过 |
| size 相同 + 无 hash | 跳过 |

**CloudToLocal 方向：** 对称逻辑。

### 7.4 删除策略

- 仅在 `--delete` 标志时启用
- **延迟删除**: scan 阶段只收集待删除项到 `pending_deletes`
- scan 全部完成后才串行执行删除
- 删除顺序：文件优先于目录，深层路径优先于浅层
- 云盘删除使用 `trash`（回收站），本地删除直接 `remove`

### 7.5 进度展示

使用 `indicatif` 多进度条：
- **scan spinner**: 实时显示当前扫描目录
- **overall bar**: `sync [████░░] 12/50 (24%) ↑8 ↓4`
- **task bars**: 每个传输任务独立进度条（带传输速度）
- **delete bar**: 删除阶段独立进度条

### 7.6 选项

| 选项 | 作用 |
|------|------|
| `--delete` | 删除目标中源没有的文件 |
| `--upload-only` | 跳过所有下载操作 |
| `--download-only` | 跳过所有上传操作 |
| `--upload-only` 与 `--download-only` 互斥 |

---

## 8. 配置管理 (config)

### 8.1 子命令

```
yun139 config show                    # 显示当前配置
yun139 config token <value>           # 设置 Token
yun139 config parallel <n>            # 设置并行数
yun139 config log <level|path|"">     # 设置日志
yun139 config exclude [add|rm|reset]  # 管理排除列表
yun139 config reset                   # 删除配置文件
```

### 8.2 Token 更新保留策略

设置新 token 时自动保留旧配置的：parallel、log_level、log_file、personal_cloud_host

### 8.3 Token 过期检测

从 token 解码中提取过期时间戳，当 `expire - now < 24h` 时提示过期警告。

---

## 9. 路径解析系统

### 9.1 云盘路径 → fileId

```
resolve_path("/photos/2024/trip.jpg")
  ├─ split → ["photos", "2024", "trip.jpg"]
  ├─ find_in_dir("/", "photos")      → fileId_1
  ├─ find_in_dir(fileId_1, "2024")   → fileId_2
  └─ find_in_dir(fileId_2, "trip.jpg") → fileId_3 (最终结果)
```

- 逐级遍历，每级通过 `list_files` + 名称匹配
- 支持分页（每页 100 条）
- 中间路径非文件夹时报错 `PathNotFound`

### 9.2 目录确保 (ensure_dir)

类似 `mkdir -p`，逐级检查：
- 存在且是文件夹 → 继续
- 不存在 → 创建
- 存在但不是文件夹 → 报错
