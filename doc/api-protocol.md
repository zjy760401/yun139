# yun139 API 协议文档

## 1. 通信协议

- **传输协议**: HTTPS
- **数据格式**: JSON
- **请求方法**: POST（所有 API）、GET（下载）、PUT（上传 OSS）
- **认证方式**: `Authorization: Basic <base64>` 请求头

---

## 2. 请求头

所有 API 请求（非 OSS）均携带以下头部：

| 头部 | 值 | 说明 |
|------|---|------|
| `Authorization` | `Basic <base64>` | 认证凭据 |
| `Content-Type` | `application/json;charset=UTF-8` | 请求体格式 |
| `mcloud-sign` | `<ts>,<rand>,<sign>` | 请求签名（见签名算法） |
| `mcloud-version` | `7.14.0` | 客户端版本 |
| `mcloud-channel` | `1000101` | 渠道标识 |
| `mcloud-client` | `10701` | 客户端标识 |
| `mcloud-route` | `001` | 路由标识 |
| `Caller` | `web` | 调用方 |
| `Origin` | `https://yun.139.com` | CORS Origin |
| `Referer` | `https://yun.139.com/w/` | 来源页 |

---

## 3. API 端点

### 3.1 路由发现

**固定地址**: `https://user-njs.yun.139.com/user/route/qryRoutePolicy`

```json
// Request
{
  "userInfo": { "userType": 1, "accountType": 1, "accountName": "<phone>" },
  "modAddrType": 1
}

// Response
{
  "success": true,
  "data": {
    "routePolicyList": [
      { "modName": "personal", "httpsUrl": "https://personal-kd-njs.yun.139.com/hcy" }
    ]
  }
}
```

后续所有个人云 API 均使用返回的 `httpsUrl` 作为基地址（`{host}`）。

### 3.2 文件列表

**`POST {host}/file/list`**

```json
// Request
{
  "imageThumbnailStyleList": ["Small", "Large"],
  "parentFileId": "/",           // "/" 表示根目录
  "pageInfo": { "pageCursor": "", "pageSize": 100 },
  "orderBy": "updated_at",
  "orderDirection": "DESC"
}

// Response
{
  "success": true,
  "data": {
    "items": [
      {
        "fileId": "xxx",
        "name": "test.mp4",
        "size": 1048576,
        "type": "file",           // "file" 或 "folder"
        "updatedAt": "2024-01-01T00:00:00.000+08:00",
        "contentHash": "abc..."   // SHA256，文件夹为空
      }
    ],
    "nextPageCursor": "..."       // 空表示无更多
  }
}
```

### 3.3 获取下载链接

**`POST {host}/file/getDownloadUrl`**

```json
// Request
{ "fileId": "xxx" }

// Response
{
  "success": true,
  "data": {
    "url": "https://...",         // 原始下载链接
    "cdnUrl": "https://..."      // CDN 加速链接（优先使用）
  }
}
```

### 3.4 创建文件/文件夹

**`POST {host}/file/create`**

创建文件夹：
```json
{
  "parentFileId": "/",
  "name": "new_folder",
  "type": "folder",
  "fileRenameMode": "refuse"      // 同名拒绝
}
```

创建上传任务：
```json
{
  "contentHash": "<sha256>",
  "contentHashAlgorithm": "SHA256",
  "contentType": "application/octet-stream",
  "parallelUpload": false,
  "partInfos": [
    { "partNumber": 1, "partSize": 104857600, "parallelHashCtx": { "partOffset": 0 } }
  ],
  "size": 209715200,
  "parentFileId": "xxx",
  "name": "large.bin",
  "type": "file",
  "fileRenameMode": "overwrite"   // 同名覆盖
}
```

```json
// Response
{
  "success": true,
  "data": {
    "fileId": "xxx",
    "uploadId": "yyy",
    "rapidUpload": false,         // true = 秒传
    "partInfos": [ { "partNumber": 1, "uploadUrl": "https://oss..." } ]
  }
}
```

### 3.5 获取上传 URL

**`POST {host}/file/getUploadUrl`**

```json
// Request
{
  "fileId": "xxx",
  "uploadId": "yyy",
  "partInfos": [
    { "partNumber": 1, "partSize": 104857600 },
    { "partNumber": 2, "partSize": 104857600 }
  ]
}

// Response
{
  "success": true,
  "data": {
    "partInfos": [
      { "partNumber": 1, "uploadUrl": "https://oss-put-url/..." },
      { "partNumber": 2, "uploadUrl": "https://oss-put-url/..." }
    ]
  }
}
```

### 3.6 完成上传

**`POST {host}/file/complete`**

```json
{
  "contentHash": "<sha256>",
  "contentHashAlgorithm": "SHA256",
  "uploadId": "yyy",
  "fileId": "xxx"
}
```

### 3.7 删除

**移入回收站**: `POST {host}/recyclebin/batchTrash`
**永久删除**: `POST {host}/file/batchDelete`

```json
{ "fileIds": ["xxx"] }
```

### 3.8 搜索

**`POST {host}/file/search`**

```json
{
  "parentFileId": "/",
  "keyword": "photo",
  "pageInfo": { "pageCursor": "", "pageSize": 100 }
}
```

---

## 4. 通用响应格式

所有 API 响应包含统一的状态字段：

```json
{
  "success": true,         // 业务成功标志
  "code": "...",           // 错误码（success=false 时）
  "message": "...",        // 错误消息
  "data": { ... }         // 业务数据（可选）
}
```

---

## 5. OSS 文件传输

### 5.1 上传

对 `/file/create` 或 `/file/getUploadUrl` 返回的每个 `uploadUrl`：

```
PUT <uploadUrl>
Content-Type: application/octet-stream
Content-Length: <part_size>

<binary_data>
```

### 5.2 下载

对 `/file/getDownloadUrl` 返回的 CDN URL：

```
GET <cdnUrl>                          # 单流下载
GET <cdnUrl> Range: bytes=0-8388607   # 分片下载
```
