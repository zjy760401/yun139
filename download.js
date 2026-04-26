/**
 * ============================================================
 *  中国移动云盘（139云盘） —— 文件下载 API
 * ============================================================
 *
 * 从 PC 客户端 (Electron, app.asar) 反编译 + 文档交叉验证。
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
 * ============================================================
 */

// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
//  1. 获取个人云文件下载地址
// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
/**
 * 接口路径: POST /hcy/file/getDownloadUrl
 *
 * 域名来源:
 *   routerInfo 中 modName="personal" 的 httpsUrl，去掉尾部 "/hcy" 后拼接。
 *   示例: https://personal-kd.yun.139.com/hcy/file/getDownloadUrl
 *
 * 功能: 获取个人云空间中单个文件的临时下载 URL（也用于图片预览-查看原图）。
 *
 * 特殊请求头:
 *   - 若为保险箱文件 (isSafeBox=true), 需额外添加:
 *     x-yun-sbox-session-id: <保险箱会话ID>
 */
const personalGetDownloadUrl = {
  method: 'POST',
  path: '/hcy/file/getDownloadUrl',
  contentType: 'application/json;charset=utf-8',

  /**
   * 请求体参数
   */
  requestBody: {
    // ---- 必填参数 ----

    fileId: '',
    // 类型: String
    // 说明: 文件唯一标识符。
    //       在个人云文件列表中返回的 fileId 字段。

    // ---- 以下为客户端传入但服务端按需使用 ----
    // 客户端在图片预览场景中会额外携带以下字段，
    // 但核心请求只需 fileId 即可获取下载地址。

    // isSafeBox: false,
    // 类型: Boolean（前端专用标识，不会发送到服务端）
    // 说明: 标识是否为保险箱文件。若为 true，客户端会在请求头追加
    //       x-yun-sbox-session-id，然后在发送前从 body 中删除此字段。
  },

  /**
   * 响应体结构 (JSON)
   */
  response: {
    code: 'S_OK',
    // 类型: String
    // 说明: 返回码。"S_OK" 表示成功。

    message: '',
    // 类型: String
    // 说明: 返回信息描述。

    var: {
      url: '',
      // 类型: String
      // 说明: 文件临时下载 URL (带签名，有效期通常为数小时)。
      //       客户端直接使用此 URL 发起 HTTP GET 下载。
    },
  },
};


// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
//  2. 批量获取个人云文件下载地址
// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
/**
 * 接口路径: POST /file/batchGetDownloadUrl
 *
 * 域名来源: 同上 (modName="personal")。
 *           完整路径通常为 BaseURL + /file/batchGetDownloadUrl
 *           (libcloud.dylib 中发现此路径，由 C++ native SDK 调用)
 *
 * 功能: 一次请求获取多个文件的下载地址。
 *       此接口在 native 层 (libcloud.dylib) 中实现，JS 层不直接调用。
 */
const batchGetDownloadUrl = {
  method: 'POST',
  path: '/file/batchGetDownloadUrl',
  contentType: 'application/json;charset=utf-8',

  requestBody: {
    fileIds: [],
    // 类型: Array<String>
    // 说明: 文件 ID 数组，批量传入需要获取下载地址的文件 ID 列表。
  },

  response: {
    code: '',
    // 类型: String
    // 说明: 返回码

    data: [
      // 类型: Array<Object>
      // 说明: 每个元素对应一个文件的下载信息
      {
        fileId: '',
        // 类型: String
        // 说明: 文件 ID

        downloadUrl: '',
        // 类型: String
        // 说明: 临时下载 URL
      },
    ],
  },
};


// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
//  3. 获取共享群文件下载地址
// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
/**
 * 接口路径: POST /hcy/mutual/adapter/isbo/openApi/getGroupFileDownLoadURLV2
 *
 * 域名来源:
 *   routerInfo 中 modName="shareGroup" 的 httpsUrl，去掉尾部 "/hcy/mutual/adapter" 后拼接。
 *   或直接使用: https://ose1.caiyun.feixin.10086.cn:8542/isbo/openApi/getGroupFileDownLoadURLV2
 *
 * 功能: 获取共享群(互助群)中文件的下载地址。
 */
const groupGetDownloadUrl = {
  method: 'POST',
  path: '/hcy/mutual/adapter/isbo/openApi/getGroupFileDownLoadURLV2',
  contentType: 'application/json;charset=utf-8',

  requestBody: {
    // 以下参数从客户端图片预览调用中提取（jsonRequestMission）

    userID: '',
    // 类型: String / Number
    // 说明: 当前登录用户 ID (window.userId)

    type: 0,
    // 类型: Number
    // 说明: 业务类型标识。
    //   0 = 个人云, 1 = 群组(共享群), 2 = 家庭云, 3 = 相册, 4 = 保险箱
    //   此处共享群通常传 1

    id: '',
    // 类型: String
    // 说明: 文件唯一标识 (contentID / fileId)

    name: '',
    // 类型: String
    // 说明: 文件名称

    size: 0,
    // 类型: Number
    // 说明: 文件大小（字节）

    path: '',
    // 类型: String
    // 说明: 文件在云空间中的完整路径

    businessType: 0,
    // 类型: Number
    // 说明: 源业务类型，与 type 含义一致

    cloudID: '',
    // 类型: String
    // 说明: 群组 ID (groupID)。共享群场景必填。

    catalogType: 0,
    // 类型: Number
    // 说明: 目录类型。0=普通文件/文件夹, 1=相册

    account: '',
    // 类型: String
    // 说明: 当前登录账号（手机号）(window.userAccount)
  },

  response: {
    code: 'S_OK',
    // 类型: String

    var: {
      url: '',
      // 类型: String
      // 说明: 临时下载 URL
    },
  },
};


// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
//  4. 获取家庭云文件下载地址
// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
/**
 * 接口路径: POST /hcy/family/adapter/andAlbum/openApi/getFileDownLoadURLV2
 *
 * 域名来源:
 *   routerInfo 中 modName="family" 的 httpsUrl，去掉尾部 "/hcy/family/adapter" 后拼接。
 *   或直接使用: https://photo.caiyun.feixin.10086.cn:443/andAlbum/openApi/getFileDownLoadURLV2
 *
 * 功能: 获取家庭云中文件的下载地址。
 */
const familyGetDownloadUrl = {
  method: 'POST',
  path: '/hcy/family/adapter/andAlbum/openApi/getFileDownLoadURLV2',
  contentType: 'application/json;charset=utf-8',

  requestBody: {
    // 参数结构与 groupGetDownloadUrl 基本一致

    userID: '',
    // 类型: String / Number
    // 说明: 用户 ID

    type: 2,
    // 类型: Number
    // 说明: 业务类型。家庭云场景传 2

    id: '',
    // 类型: String
    // 说明: 文件唯一标识

    name: '',
    // 类型: String
    // 说明: 文件名称

    size: 0,
    // 类型: Number
    // 说明: 文件大小（字节）

    path: '',
    // 类型: String
    // 说明: 文件路径

    businessType: 2,
    // 类型: Number
    // 说明: 源业务类型，家庭云传 2

    cloudID: '',
    // 类型: String
    // 说明: 家庭云 ID (cloudID / familyId)

    catalogType: 0,
    // 类型: Number
    // 说明: 目录类型。0=普通, 1=相册

    account: '',
    // 类型: String
    // 说明: 当前登录账号（手机号）
  },

  response: {
    code: 'S_OK',
    var: {
      url: '',
      // 类型: String
      // 说明: 临时下载 URL
    },
  },
};


// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
//  5. 获取挂载盘文件下载地址
// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
/**
 * 接口路径: POST /mount/file/getDownloadUrl
 *
 * 域名来源:
 *   routerInfo 中 modName="mount" 的 httpsUrl，去掉尾部 "/mount" 后拼接。
 *
 * 功能: 获取挂载盘中文件的下载地址。
 */
const mountGetDownloadUrl = {
  method: 'POST',
  path: '/mount/file/getDownloadUrl',
  contentType: 'application/json;charset=utf-8',

  requestBody: {
    fileId: '',
    // 类型: String
    // 说明: 挂载盘中的文件唯一标识
  },

  response: {
    code: 'S_OK',
    var: {
      url: '',
      // 类型: String
      // 说明: 临时下载 URL
    },
  },
};


// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
//  6. 客户端下载任务提交（Native SDK 层）
// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
/**
 * 这不是一个 HTTP API，而是 Electron 前端调用 native SDK 的参数结构。
 * 通过 apiNative.transfer.getTransferApi('download', params, callback) 调用。
 *
 * 实际下载由 C++ 原生进程 uploadDownload 执行：
 *   - 先调用 /file/getDownloadUrl 获取 URL
 *   - 然后分片下载（支持断点续传）
 *
 * 此处记录参数结构，供参考完整下载流程。
 */
const nativeDownloadParams = {
  // --- 基本参数 ---

  userId: 0,
  // 类型: Number
  // 说明: 传输列表用户 ID (Number(localStorage.getItem('transferUserId')))

  fileId: '',
  // 类型: String
  // 说明: 文件 ID (cutover 割接模式下使用)

  name: '',
  // 类型: String
  // 说明: 文件名称

  size: 0,
  // 类型: Number
  // 说明: 文件大小（字节）

  type: 0,
  // 类型: Number
  // 说明: 业务类型。
  //   0 = 个人云, 1 = 群组, 2 = 家庭, 3 = 相册, 4 = 保险箱

  ids: [
    // 类型: Array<Object>
    // 说明: 下载文件列表。支持批量下载多个文件。
    {
      id: '',
      // 类型: String
      // 说明: 文件 / 文件夹 ID (contentID / fileId)

      type: 0,
      // 类型: Number
      // 说明: 0=文件, 1=文件夹

      name: '',
      // 类型: String
      // 说明: 文件名

      size: 0,
      // 类型: Number
      // 说明: 文件大小；文件夹传 0

      thumbnailUrl: '',
      // 类型: String
      // 说明: 缩略图 URL（可选）

      // --- 非个人云场景(businessType!=0)时需要以下字段 ---

      businessType: 0,
      // 类型: Number
      // 说明: 源业务类型。0=个人云, 1=家庭, 2=群组, 3=相册

      cloudID: '',
      // 类型: String
      // 说明: 家庭云 ID 或群组 ID

      catalogType: 0,
      // 类型: Number
      // 说明: 目录类型。0=普通, 1=相册

      path: '',
      // 类型: String
      // 说明: 文件全路径

      md5: '',
      // 类型: String
      // 说明: 文件 MD5 值（可选）
    },
  ],

  // --- 下载路径相关（可选） ---

  defaultDownloadPath: '',
  // 类型: String
  // 说明: 默认下载目录路径

  useDefaultPath: false,
  // 类型: Boolean
  // 说明: 是否使用默认下载路径（不弹出选择对话框）

  // --- 保险箱场景 ---

  sessionId: '',
  // 类型: String
  // 说明: 保险箱会话 ID。仅 type=4 时需要。
};


// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
//  7. Native 下载流程（libcloud.dylib 底层 API 序列）
// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
/**
 * 完整的底层下载流程（从 libcloud.dylib 反编译得到）：
 *
 * 1. POST /file/getDownloadUrl
 *    请求: { fileId: "..." }
 *    响应: { code: "...", data: { downloadUrl: "https://..." } }
 *
 * 2. 分片下载
 *    Native SDK 根据文件大小决定分片策略：
 *    - 每个分片通过 HTTP GET 请求 downloadUrl，使用 Range 头指定字节范围
 *    - 数据库表记录每个分片状态:
 *        partIndex    - 分片索引
 *        partSize     - 分片大小
 *        rangeBegin   - Range 起始字节
 *        rangeEnd     - Range 结束字节
 *        downloadUrl  - 下载 URL
 *        writeBytes   - 已写入字节数（用于断点续传）
 *        state        - 分片状态
 *
 * 3. 所有分片完成后合并文件。
 */


// ============================================================
//  导出
// ============================================================
module.exports = {
  personalGetDownloadUrl,
  batchGetDownloadUrl,
  groupGetDownloadUrl,
  familyGetDownloadUrl,
  mountGetDownloadUrl,
  nativeDownloadParams,
};
