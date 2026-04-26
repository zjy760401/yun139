/**
 * ============================================================
 *  中国移动云盘（139云盘） —— 文件上传 API
 * ============================================================
 *
 * 从 PC 客户端 (Electron, app.asar) 反编译 + libcloud.dylib 逆向分析。
 * 客户端版本路径: /Applications/中国移动云盘.app/
 *
 * 【基础说明】
 *  - 所有 API 均为 POST，Content-Type: application/json;charset=utf-8
 *  - 需要在请求头中携带 Authorization: Basic base64("pc:" + account + ":" + authToken)
 *  - BaseURL 在登录后由 UserInfo.routerInfo 动态下发，按 modName 匹配。
 *    例如 modName="personal" 对应 /hcy/* 路径的 BaseURL。
 *  - 公共请求头（cutover 模式）:
 *      x-yun-app-channel: "10301000"(Mac) / "10200153"(Win)
 *      x-yun-api-version: "v1"
 *      x-yun-module-type: "100"
 *      x-yun-op-type: "1"
 *      x-yun-svc-type: "1"
 *      x-yun-client-info: <设备信息>
 *      x-yun-device-id: <机器码>
 *      x-ExpRoute-Code: "routeCode=<手机号>,type=2"
 *
 * 【上传完整流程】
 *  Step 1: /file/create          — 创建文件（获取 uploadId + 分片上传地址）
 *  Step 2: PUT uploadUrl          — 逐片上传文件数据到 OSS
 *  Step 3: /file/getUploadUrl     — （可选）分片上传地址过期时重新获取
 *  Step 4: /file/listUploadedParts — （可选）查询已上传分片（断点续传）
 *  Step 5: /file/complete         — 通知服务端所有分片上传完成，合并文件
 *  Step 6: /file/commit           — （可选）提交文件最终确认
 *
 *  秒传: 在 Step 1 中如果 contentHash 匹配到服务端已有文件，
 *        响应 isNeedUpload=false / rapidUpload=true，可跳过 Step 2-5。
 *
 * ============================================================
 */

// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
//  Step 1: 创建文件 / 初始化上传
// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
/**
 * 接口路径: POST /hcy/file/create
 *
 * 域名来源:
 *   routerInfo 中 modName="personal" 的 httpsUrl，去掉尾部 "/hcy" 后拼接。
 *   示例: https://personal-kd.yun.139.com/hcy/file/create
 *
 * 功能:
 *   在指定目录下创建文件（或文件夹）。
 *   对于文件上传，服务端返回 uploadId 和各分片的 uploadUrl，
 *   客户端随后使用这些 URL 将文件数据 PUT 到对象存储。
 *   若服务端检测到 contentHash 已存在，可实现"秒传"。
 *
 * 挂载盘变体: POST /mount/file/create (modName="mount")
 *
 * 特殊请求头:
 *   - 保险箱文件需额外添加: x-yun-sbox-session-id: <保险箱会话ID>
 */
const fileCreate = {
  method: 'POST',
  path: '/hcy/file/create',
  contentType: 'application/json;charset=utf-8',

  requestBody: {
    // ━━━━━━━━━━ 必填参数 ━━━━━━━━━━

    parentFileId: '',
    // 类型: String
    // 说明: 父文件夹 ID。上传到根目录时传 "/"。
    //       来源于文件列表接口返回的 fileId。
    //       注意: 旧版根目录 ID 形如 "00019700101000000001"，
    //       客户端会自动映射为 "/" 。

    name: '',
    // 类型: String
    // 说明: 文件名（含后缀），例如 "report.pdf"。
    //       文件夹则为文件夹名称。

    type: 'file',
    // 类型: String
    // 说明: 创建类型。
    //   "file"   — 创建文件（上传场景）
    //   "folder" — 创建文件夹

    size: 0,
    // 类型: Number
    // 说明: 文件大小（字节）。文件夹可传 0。

    // ━━━━━━━━━━ 内容校验（秒传关键参数） ━━━━━━━━━━

    contentHash: '',
    // 类型: String
    // 说明: 文件内容的哈希值。
    //       通常为文件整体的 SHA1 或 SHA256 值（大写十六进制字符串）。
    //       服务端用此值判断是否可以"秒传"（rapid upload）——
    //       如果云端已有相同哈希的文件，无需再次上传数据。

    contentHashAlgorithm: '',
    // 类型: String
    // 说明: 哈希算法名称，与 contentHash 配套使用。
    //       常见值: "sha1", "sha256"

    proofCode: '',
    // 类型: String
    // 说明: 文件所有权证明码。
    //       客户端根据 authToken 和文件内容特定偏移处的字节计算得出。
    //       服务端用此验证客户端确实持有该文件（防止恶意秒传）。

    proofVersion: '',
    // 类型: String
    // 说明: proofCode 的算法版本号，通常为 "v1"。

    // ━━━━━━━━━━ 分片信息 ━━━━━━━━━━

    partInfos: [],
    // 类型: Array<Object>
    // 说明: 分片信息数组。每个元素描述一个待上传的分片。
    //       客户端根据文件大小预先计算分片，每片通常 4~16MB。
    //       服务端根据此数组返回每片对应的 uploadUrl。
    //
    // 元素结构:
    //   {
    //     partNumber: 1,          // Number - 分片序号，从 1 开始递增
    //   }
    //
    // 示例 (100MB 文件, 每片 16MB):
    //   [
    //     { partNumber: 1 },
    //     { partNumber: 2 },
    //     { partNumber: 3 },
    //     { partNumber: 4 },
    //     { partNumber: 5 },
    //     { partNumber: 6 },
    //     { partNumber: 7 },
    //   ]

    // ━━━━━━━━━━ 可选参数 ━━━━━━━━━━

    fileId: '',
    // 类型: String
    // 说明: 文件 ID。创建新文件时留空，服务端会自动生成。
    //       断点续传或覆盖上传时可指定已有 fileId。

    parentPath: '',
    // 类型: String
    // 说明: 父文件夹路径。辅助定位字段，部分场景可选。

    description: '',
    // 类型: String
    // 说明: 文件描述信息（可选）。

    contentType: '',
    // 类型: String
    // 说明: 文件的 MIME 类型，如 "application/pdf", "image/jpeg"。
    //       不传时服务端可根据文件名后缀自动推断。

    expireSec: 0,
    // 类型: Number
    // 说明: 上传 URL 的过期时间（秒）。
    //       服务端返回的 uploadUrl 在此时间后失效，
    //       需调用 /file/getUploadUrl 重新获取。

    fileRenameMode: '',
    // 类型: String
    // 说明: 文件重名处理策略。
    //       常见值:
    //         "auto_rename" — 自动重命名（如 "file(1).txt"）
    //         "refuse"      — 拒绝（返回错误）
    //         "overwrite"   — 覆盖同名文件

    localCreatedAt: '',
    // 类型: String (ISO 8601)
    // 说明: 文件在本地的创建时间。
    //       格式如 "2024-01-15T10:30:00.000Z"
    //       用于保留文件原始时间元数据。

    localUpdatedAt: '',
    // 类型: String (ISO 8601)
    // 说明: 文件在本地的最后修改时间。

    mediaMetaInfo: null,
    // 类型: Object | null
    // 说明: 媒体文件元信息（图片 EXIF、视频时长等）。
    //       由客户端解析本地文件后填入，非媒体文件可传 null。

    parallelUpload: false,
    // 类型: Boolean
    // 说明: 是否启用并行上传。
    //       true  — 多个分片可同时上传
    //       false — 按顺序逐片上传

    formUpload: false,
    // 类型: Boolean
    // 说明: 是否使用 form 表单上传模式（小文件场景）。

    storyVideoFile: null,
    // 类型: Object | null
    // 说明: 故事视频文件标识（相册功能相关），普通上传传 null。
  },

  /**
   * 响应体结构 (JSON)
   */
  response: {
    code: '',
    // 类型: String
    // 说明: 返回码

    data: {
      fileId: '',
      // 类型: String
      // 说明: 服务端分配的文件 ID

      uploadId: '',
      // 类型: String
      // 说明: 本次上传会话 ID。后续 getUploadUrl / complete 等接口都需要此值。
      //       若为秒传，此字段可能为空。

      isNeedUpload: true,
      // 类型: Boolean
      // 说明: 是否需要实际上传文件数据。
      //   true  — 需要继续上传各分片
      //   false — 秒传成功，文件已存在，无需上传

      rapidUpload: false,
      // 类型: Boolean
      // 说明: 是否命中秒传。
      //   true  — 秒传成功
      //   false — 正常上传

      exist: false,
      // 类型: Boolean
      // 说明: 同名文件是否已存在

      partInfos: [],
      // 类型: Array<Object>
      // 说明: 各分片的上传地址信息。与请求中的 partInfos 一一对应。
      //
      // 元素结构:
      //   {
      //     partNumber: 1,        // Number  - 分片序号
      //     uploadUrl: "https://..."  // String  - 该分片的上传地址 (PUT)
      //                           //           通常是对象存储 (OSS) 预签名 URL
      //   }
    },
  },
};


// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
//  Step 2: 上传分片数据
// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
/**
 * 方法: PUT <uploadUrl>
 *
 * 这不是云盘自身的 API，而是将分片数据直接 PUT 到 OSS 预签名 URL。
 * uploadUrl 来自 Step 1（/file/create）或 Step 3（/file/getUploadUrl）的响应。
 *
 * 功能: 上传单个分片的二进制数据。
 */
const uploadPart = {
  method: 'PUT',
  url: '<从 fileCreate 响应的 partInfos[n].uploadUrl 获取>',

  requestHeaders: {
    'Content-Type': 'application/octet-stream',
    // 说明: 固定为二进制流

    'Content-Length': 0,
    // 类型: Number
    // 说明: 当前分片的实际字节数
  },

  requestBody: '<该分片的原始二进制数据 (Buffer / ArrayBuffer)>',
  // 说明: 从本地文件中读取的该分片字节范围:
  //   分片 N 的字节范围: [(N-1)*partSize, min(N*partSize, fileSize))
  //   客户端默认分片大小配置来自 libcloud.dylib 中的 UPLOAD SLICE SIZE

  /**
   * 响应
   */
  response: {
    // HTTP 200 OK 表示成功
    // 响应头中包含:

    ETag: '',
    // 类型: String (在 HTTP 响应头中)
    // 说明: OSS 返回的该分片 ETag 值。
    //       客户端需要保存此值，在 Step 5 (/file/complete) 中提交。
    //       libcloud.dylib 会校验本地 etag 与远端 etag 是否一致。
  },
};


// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
//  Step 3: 重新获取分片上传地址（可选）
// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
/**
 * 接口路径: POST /file/getUploadUrl
 *
 * 域名来源: 同 /hcy/file/create，完整路径前缀根据 routerInfo 拼接。
 *           libcloud.dylib 中使用相对路径 /file/getUploadUrl
 *           对应实际: https://<personal-base>/hcy/file/getUploadUrl
 *           或动态拼接: https://<personal-base>/dynamic/file/getUploadUrl
 *
 * 功能:
 *   当 uploadUrl 过期后，使用此接口重新获取。
 *   无需重新创建文件，只需提供 fileId 和 uploadId。
 *
 * libcloud.dylib 日志: "need get new upload url"
 */
const fileGetUploadUrl = {
  method: 'POST',
  path: '/file/getUploadUrl',
  // 注: libcloud.dylib 中同时存在 /dynamic/file/getUploadUrl 变体
  contentType: 'application/json;charset=utf-8',

  requestBody: {
    fileId: '',
    // 类型: String
    // 说明: 文件 ID（来自 Step 1 创建响应）

    uploadId: '',
    // 类型: String
    // 说明: 上传会话 ID（来自 Step 1 创建响应）

    partInfos: [],
    // 类型: Array<Object>
    // 说明: 需要重新获取上传地址的分片列表。
    //
    // 元素结构:
    //   {
    //     partNumber: 1,    // Number - 分片序号
    //   }

    expireSec: 0,
    // 类型: Number
    // 说明: 新的上传 URL 过期时间（秒），可选
  },

  response: {
    code: '',
    // 类型: String

    data: {
      partInfos: [],
      // 类型: Array<Object>
      // 说明: 刷新后的分片上传地址
      //
      // 元素结构:
      //   {
      //     partNumber: 1,
      //     uploadUrl: "https://..."   // 新的预签名上传 URL
      //   }
    },
  },
};


// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
//  Step 4: 查询已上传分片（断点续传）
// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
/**
 * 接口路径: POST /file/listUploadedParts
 *
 * 域名来源: 同上。
 *
 * 功能:
 *   查询指定上传会话中已成功上传的分片列表。
 *   用于断点续传场景——客户端可跳过已上传的分片，只上传剩余部分。
 */
const fileListUploadedParts = {
  method: 'POST',
  path: '/file/listUploadedParts',
  contentType: 'application/json;charset=utf-8',

  requestBody: {
    fileId: '',
    // 类型: String
    // 说明: 文件 ID

    uploadId: '',
    // 类型: String
    // 说明: 上传会话 ID
  },

  response: {
    code: '',
    // 类型: String

    data: {
      partInfos: [],
      // 类型: Array<Object>
      // 说明: 已成功上传的分片列表
      //
      // 元素结构:
      //   {
      //     partNumber: 1,    // Number - 分片序号
      //     partSize: 16777216, // Number - 分片大小（字节）
      //     etag: "...",      // String - 该分片的 ETag
      //   }
    },
  },
};


// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
//  Step 5: 完成上传（合并分片）
// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
/**
 * 接口路径: POST /file/complete
 *
 * 域名来源: 同上。
 *           libcloud.dylib 中同时存在 /dynamic/file/complete 变体。
 *
 * 功能:
 *   通知服务端所有分片已上传完成，触发服务端合并文件。
 *
 * libcloud.dylib 日志: "request send failing(upload complete)"
 */
const fileComplete = {
  method: 'POST',
  path: '/file/complete',
  contentType: 'application/json;charset=utf-8',

  requestBody: {
    fileId: '',
    // 类型: String
    // 说明: 文件 ID（来自 Step 1 响应）

    uploadId: '',
    // 类型: String
    // 说明: 上传会话 ID（来自 Step 1 响应）
  },

  response: {
    code: '',
    // 类型: String
    // 说明: 返回码

    message: '',
    // 类型: String

    data: {
      fileId: '',
      // 类型: String
      // 说明: 最终文件 ID

      name: '',
      // 类型: String
      // 说明: 文件名

      size: 0,
      // 类型: Number
      // 说明: 文件大小

      contentHash: '',
      // 类型: String
      // 说明: 服务端计算的文件哈希值

      status: '',
      // 类型: String
      // 说明: 文件状态
    },
  },
};


// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
//  Step 6: 提交文件确认（可选）
// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
/**
 * 接口路径: POST /file/commit
 *
 * 域名来源: 同上。
 *
 * 功能:
 *   在某些场景下，complete 之后还需要 commit 来最终确认文件入库。
 *   libcloud.dylib 中发现此路径，属于可选步骤。
 */
const fileCommit = {
  method: 'POST',
  path: '/file/commit',
  contentType: 'application/json;charset=utf-8',

  requestBody: {
    fileId: '',
    // 类型: String
    // 说明: 文件 ID
  },

  response: {
    code: '',
    // 类型: String

    data: {
      fileId: '',
      // 类型: String

      status: '',
      // 类型: String
    },
  },
};


// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
//  批量创建文件
// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
/**
 * 接口路径: POST /file/batchCreate
 *
 * 域名来源: 同上。libcloud.dylib 中发现此路径。
 *
 * 功能: 一次请求创建多个文件/文件夹。
 *       参数结构为 fileCreate.requestBody 的数组形式。
 */
const fileBatchCreate = {
  method: 'POST',
  path: '/file/batchCreate',
  contentType: 'application/json;charset=utf-8',

  requestBody: {
    items: [],
    // 类型: Array<Object>
    // 说明: 每个元素结构同 fileCreate.requestBody
  },
};


// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
//  批量完成上传
// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
/**
 * 接口路径: POST /file/batchComplete
 *
 * 域名来源: 同上。libcloud.dylib 中发现此路径。
 *
 * 功能: 一次请求完成多个文件的上传。
 */
const fileBatchComplete = {
  method: 'POST',
  path: '/file/batchComplete',
  contentType: 'application/json;charset=utf-8',

  requestBody: {
    items: [],
    // 类型: Array<Object>
    // 说明: 每个元素包含 { fileId, uploadId }
  },
};


// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
//  检查文件是否存在（秒传预检）
// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
/**
 * 接口路径: POST /file/checkExists
 *
 * 域名来源: 同上。libcloud.dylib 中发现此路径。
 *
 * 功能:
 *   在正式创建文件前，先检查云端是否已存在相同内容的文件。
 *   可用于提前判断是否可以秒传。
 */
const fileCheckExists = {
  method: 'POST',
  path: '/file/checkExists',
  contentType: 'application/json;charset=utf-8',

  requestBody: {
    parentFileId: '',
    // 类型: String
    // 说明: 父文件夹 ID

    name: '',
    // 类型: String
    // 说明: 文件名

    contentHash: '',
    // 类型: String
    // 说明: 文件内容哈希值

    contentHashAlgorithm: '',
    // 类型: String
    // 说明: 哈希算法名称

    size: 0,
    // 类型: Number
    // 说明: 文件大小
  },

  response: {
    code: '',
    // 类型: String

    data: {
      exist: false,
      // 类型: Boolean
      // 说明: 文件是否已存在

      fileId: '',
      // 类型: String
      // 说明: 若已存在，返回已有文件的 ID
    },
  },
};


// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
//  Native 上传任务提交（Electron → Native SDK）
// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
/**
 * 这不是 HTTP API，而是 Electron 前端调用 native SDK 的参数结构。
 * 通过 apiNative.transfer.getTransferApi('upload', params, callback) 调用。
 *
 * 实际上传由 C++ 原生进程 uploadDownload 执行，
 * 内部调用上述 /file/create → PUT → /file/complete 流程。
 */
const nativeUploadParams = {
  userId: 0,
  // 类型: Number
  // 说明: 传输列表用户 ID (Number(localStorage.getItem('transferUserId')))

  type: 0,
  // 类型: Number
  // 说明: 业务类型。
  //   0 = 个人云, 1 = 群组, 2 = 家庭, 3 = 相册, 4 = 保险箱

  parentFileId: '',
  // 类型: String
  // 说明: 目标文件夹 ID

  files: [],
  // 类型: Array<String>
  // 说明: 本地文件路径数组。
  //       例如: ["/Users/ian/Desktop/report.pdf", "/Users/ian/Desktop/photo.jpg"]
};


// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
//  完整上传流程示意（伪代码）
// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
/**
 * async function uploadFile(localPath, parentFileId, authHeaders) {
 *
 *   // 1. 读取本地文件，计算 hash
 *   const fileBuffer = fs.readFileSync(localPath);
 *   const fileSize = fileBuffer.length;
 *   const contentHash = sha1(fileBuffer).toUpperCase();
 *   const fileName = path.basename(localPath);
 *
 *   // 2. 计算分片
 *   const PART_SIZE = 16 * 1024 * 1024; // 16MB per part
 *   const partCount = Math.ceil(fileSize / PART_SIZE);
 *   const partInfos = Array.from({ length: partCount }, (_, i) => ({
 *     partNumber: i + 1,
 *   }));
 *
 *   // 3. 调用 /hcy/file/create
 *   const createRes = await post('/hcy/file/create', {
 *     parentFileId,
 *     name: fileName,
 *     type: 'file',
 *     size: fileSize,
 *     contentHash,
 *     contentHashAlgorithm: 'sha1',
 *     partInfos,
 *     fileRenameMode: 'auto_rename',
 *     parallelUpload: true,
 *   }, authHeaders);
 *
 *   // 4. 检查是否秒传
 *   if (!createRes.data.isNeedUpload || createRes.data.rapidUpload) {
 *     console.log('秒传成功!', createRes.data.fileId);
 *     return createRes.data;
 *   }
 *
 *   const { fileId, uploadId } = createRes.data;
 *
 *   // 5. 逐片上传
 *   for (const partInfo of createRes.data.partInfos) {
 *     const start = (partInfo.partNumber - 1) * PART_SIZE;
 *     const end = Math.min(partInfo.partNumber * PART_SIZE, fileSize);
 *     const partData = fileBuffer.slice(start, end);
 *
 *     const putRes = await fetch(partInfo.uploadUrl, {
 *       method: 'PUT',
 *       headers: { 'Content-Type': 'application/octet-stream' },
 *       body: partData,
 *     });
 *
 *     // 保存 ETag（可选，complete 时可能需要）
 *     const etag = putRes.headers.get('ETag');
 *   }
 *
 *   // 6. 通知服务端完成
 *   const completeRes = await post('/file/complete', {
 *     fileId,
 *     uploadId,
 *   }, authHeaders);
 *
 *   console.log('上传完成!', completeRes.data);
 *   return completeRes.data;
 * }
 */


// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
//  libcloud.dylib 中发现的所有上传相关路径汇总
// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
/**
 * /file/create              — 创建文件/初始化上传
 * /file/batchCreate         — 批量创建文件
 * /file/getUploadUrl        — 获取/刷新分片上传地址
 * /file/listUploadedParts   — 查询已上传分片
 * /file/complete            — 完成上传（合并分片）
 * /file/batchComplete       — 批量完成上传
 * /file/commit              — 提交文件确认
 * /file/checkExists         — 检查文件是否已存在
 *
 * /dynamic/file/create      — 动态资源创建（相册等场景）
 * /dynamic/file/getUploadUrl — 动态资源上传地址
 * /dynamic/file/complete    — 动态资源上传完成
 *
 * 以上路径会根据业务场景拼接不同前缀:
 *   个人云: /hcy/file/...
 *   挂载盘: /mount/file/...
 *   动态:   /dynamic/file/...
 */


// ============================================================
//  导出
// ============================================================
module.exports = {
  fileCreate,
  uploadPart,
  fileGetUploadUrl,
  fileListUploadedParts,
  fileComplete,
  fileCommit,
  fileBatchCreate,
  fileBatchComplete,
  fileCheckExists,
  nativeUploadParams,
};
