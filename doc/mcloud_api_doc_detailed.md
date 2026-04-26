# Detailed API document (with parameter snippets, best-effort)

Note: object literals extracted heuristically from function bodies.

## /hcy/videoPreview/getPreviewInfo

- Functions: if

- Files: out/renderer/api/apiServer.js

- Method (guessed): POST (data present)

- Sample line: 61

- 参数示例未在函数中以静态对象形式发现；可能动态构造。

## /orchestration/file/video/v1.0/queryFlvAddr

- Functions: if, getRsaPublicKey, dynamicDeclare

- Files: out/renderer/api/apiServer.js

- Method (guessed): POST (data present)

- Sample line: 91

- 参数示例未在函数中以静态对象形式发现；可能动态构造。

## /tellin/logout.do

- Functions: quitOutLogin

- Files: out/renderer/api/apiServer.js

- Method (guessed): POST (data present)

- Sample line: 136


```javascript
{
        root: {
          msisdn: window.userAccount || window.UserInfo.account,
          token: window.UserInfo?.token || "",
        },
      }
```


## /user/auth/refreshToken

- Functions: refreshToken

- Files: out/renderer/api/apiServer.js

- Method (guessed): POST (data present)

- Sample line: 145

- 参数示例未在函数中以静态对象形式发现；可能动态构造。

## /tellin/getDyncPasswd.do

- Functions: if, getSafeLoginVerifyCode

- Files: out/renderer/api/apiServer.js

- Method (guessed): POST (data present)

- Sample line: 170

- 参数示例未在函数中以静态对象形式发现；可能动态构造。

## /user/thirdlogin

- Functions: login, simLogin, if, QRLogin

- Files: out/renderer/api/apiServer.js

- Method (guessed): POST (data present)

- Sample line: 204


```javascript
{
        clientType,
        cpid,
        dycpwd: params.verifyCode,
        extInfo: {
          ifOpenAccount: 1,
        },
        loginMode: "0",
        msisdn: params.username,
        pintype: "5",
        verType: "2",
        version: window.appVersion,
      }
```


## /tellin/querySendSingleMsgResult.do

- Functions: if

- Files: out/renderer/api/apiServer.js

- Method (guessed): POST (data present)

- Sample line: 293

- 参数示例未在函数中以静态对象形式发现；可能动态构造。

## /tellin/verfycode.do

- Functions: getVerfyCode

- Files: out/renderer/api/apiServer.js

- Method (guessed): POST (data present)

- Sample line: 461


```javascript
{
        root: {
          account: account,
          type: 2,
        },
      }
```


## /tellin/verifyDyncPasswd.do

- Functions: verifyDyncPasswd

- Files: out/renderer/api/apiServer.js

- Method (guessed): POST (data present)

- Sample line: 508


```javascript
{
        root: {
          account: params.username,
          secinfo: Utils.safeVerifyCodeEncrypt(
            params.smsCheckRandom + params.username + params.verifyCode
          ),
          recordID: protectRecordId,
          reqType: 9,
        },
      }
```


## /user/querySpecTokenV2

- Functions: getCloudTicket, getPublicRSAKey, openWebBrowser, if, silverAuthQuery, memberAuthQuery, getEditVision, getSafeBoxLoginStatus

- Files: out/renderer/api/apiServer.js

- Method (guessed): POST (data present)

- Sample line: 705

- 参数示例未在函数中以静态对象形式发现；可能动态构造。

## /caiyun/openapi/authentication/key/getRsaPublicKey

- Functions: openAIWebChat, mountDelete, mountSerch

- Files: out/renderer/api/apiServer.js

- Method (guessed): POST (data present)

- Sample line: 778

- 参数示例未在函数中以静态对象形式发现；可能动态构造。

## /isbo2/openApi/queryMbBenefitInfoV2

- Functions: queryUserSecQuesAndSecMail, getPublicKey, safeBoxAppLogin, safeboxRefreshSession, getSafeBoxQuestions, getSafePhoneCode

- Files: out/renderer/api/apiServer.js

- Method (guessed): POST (data present)

- Sample line: 838

- 参数示例未在函数中以静态对象形式发现；可能动态构造。

## /isbo2/openApi/queryUserBenefitsV2

- Functions: getUserSecQuesAndSecMai

- Files: out/renderer/api/apiServer.js

- Method (guessed): POST (data present)

- Sample line: 874

- 参数示例未在函数中以静态对象形式发现；可能动态构造。

## /yun-note/configCenter/getEditorConfig

- Functions: getSecMailIdenCode, verSecMailIdenCode

- Files: out/renderer/api/apiServer.js

- Method (guessed): POST (data present)

- Sample line: 880

- 参数示例未在函数中以静态对象形式发现；可能动态构造。

## /hcy/safebox/status

- Functions: safeBoxVerSecQuestionV2

- Files: out/renderer/api/apiServer.js

- Method (guessed): POST (data present)

- Sample line: 892

- 参数示例未在函数中以静态对象形式发现；可能动态构造。

## /hcy/safebox/getUserSecQuesAndSecMai

- Functions: verifyPhoneIdenCode, getUserInfo

- Files: out/renderer/api/apiServer.js

- Method (guessed): POST (data present)

- Sample line: 898

- 参数示例未在函数中以静态对象形式发现；可能动态构造。

## /hcy/safebox/getPublicKey

- Functions: firstApplySecMail

- Files: out/renderer/api/apiServer.js

- Method (guessed): POST (data present)

- Sample line: 904

- 参数示例未在函数中以静态对象形式发现；可能动态构造。

## /hcy/safebox/login

- Functions: firstSetSbox

- Files: out/renderer/api/apiServer.js

- Method (guessed): POST (data present)

- Sample line: 910

- 参数示例未在函数中以静态对象形式发现；可能动态构造。

## /hcy/safebox/renewSession

- Functions: safeBoxSetAppPwdV4

- Files: out/renderer/api/apiServer.js

- Method (guessed): POST (data present)

- Sample line: 916

- 参数示例未在函数中以静态对象形式发现；可能动态构造。

## /hcy/safebox/listSysSecQuestions

- Functions: querySimLoginResult

- Files: out/renderer/api/apiServer.js

- Method (guessed): POST (data present)

- Sample line: 922

- 参数示例未在函数中以静态对象形式发现；可能动态构造。

## /hcy/safebox/applyPhoneIdenCode

- Functions: safeboxSimLogin

- Files: out/renderer/api/apiServer.js

- Method (guessed): POST (data present)

- Sample line: 928

- 参数示例未在函数中以静态对象形式发现；可能动态构造。

## /hcy/safebox/applySecMailIdenCode

- Functions: getUserAvatar

- Files: out/renderer/api/apiServer.js

- Method (guessed): POST (data present)

- Sample line: 940

- 参数示例未在函数中以静态对象形式发现；可能动态构造。

## /hcy/safebox/verifySecMailIdenCode

- Functions: getUserMemberInfo

- Files: out/renderer/api/apiServer.js

- Method (guessed): POST (data present)

- Sample line: 945

- 参数示例未在函数中以静态对象形式发现；可能动态构造。

## /hcy/safebox/verifySecQuestion

- Functions: getUserStatus

- Files: out/renderer/api/apiServer.js

- Method (guessed): POST (data present)

- Sample line: 950

- 参数示例未在函数中以静态对象形式发现；可能动态构造。

## /hcy/safebox/verifyPhoneIdenCode

- Functions: getUserRoutePolicy

- Files: out/renderer/api/apiServer.js

- Method (guessed): POST (data present)

- Sample line: 955

- 参数示例未在函数中以静态对象形式发现；可能动态构造。

## /hcy/safebox/firstSetSbox

- Functions: getSurplusNum

- Files: out/renderer/api/apiServer.js

- Method (guessed): POST (data present)

- Sample line: 967

- 参数示例未在函数中以静态对象形式发现；可能动态构造。

## /hcy/file/list

- Functions: if

- Files: out/renderer/api/apiServer.js

- Method (guessed): POST (data present)

- Sample line: 1044

- 参数示例未在函数中以静态对象形式发现；可能动态构造。

## /richlifeApp/devapp/ICatalog

- Functions: if, delCollection, getAlbumSwitch, getPersonList

- Files: out/renderer/api/apiServer.js

- Method (guessed): POST (data present)

- Sample line: 1068

- 参数示例未在函数中以静态对象形式发现；可能动态构造。

## /hcy/bookmark/listFiles

- Functions: if, addCollection

- Files: out/renderer/api/apiServer.js

- Method (guessed): POST (data present)

- Sample line: 1174

- 参数示例未在函数中以静态对象形式发现；可能动态构造。

## /hcy/bookmark/addFiles

- Functions: getLocationList, getThingList, if

- Files: out/renderer/api/apiServer.js

- Method (guessed): POST (data present)

- Sample line: 1259

- 参数示例未在函数中以静态对象形式发现；可能动态构造。

## /hcy/bookmark/removeFiles

- Functions: renamePersonAlbum, getAlbumScope

- Files: out/renderer/api/apiServer.js

- Method (guessed): POST (data present)

- Sample line: 1281

- 参数示例未在函数中以静态对象形式发现；可能动态构造。

## /album/timeline/intelligent/user/ai/get

- Functions: setAlbumScope

- Files: out/renderer/api/apiServer.js

- Method (guessed): POST (data present)

- Sample line: 1291

- 参数示例未在函数中以静态对象形式发现；可能动态构造。

## /album/classify/thing/list

- Functions: getAlbum

- Files: out/renderer/api/apiServer.js

- Method (guessed): POST (data present)

- Sample line: 1316

- 参数示例未在函数中以静态对象形式发现；可能动态构造。

## /album/timeline/user/range/get

- Functions: getAlbumCache

- Files: out/renderer/api/apiServer.js

- Method (guessed): POST (data present)

- Sample line: 1340

- 参数示例未在函数中以静态对象形式发现；可能动态构造。

## /album/timeline/get

- Functions: if

- Files: out/renderer/api/apiServer.js

- Method (guessed): POST (data present)

- Sample line: 1365

- 参数示例未在函数中以静态对象形式发现；可能动态构造。

## /hcy/file/search

- Functions: if

- Files: out/renderer/api/apiServer.js

- Method (guessed): POST (data present)

- Sample line: 1388

- 参数示例未在函数中以静态对象形式发现；可能动态构造。

## /mount/videoPreview/getPreviewInfo

- Functions: if

- Files: out/renderer/api/apiServer.js

- Method (guessed): POST (data present)

- Sample line: 1458

- 参数示例未在函数中以静态对象形式发现；可能动态构造。

## /hcy/revision/listRevision

- Functions: if, getTypeList

- Files: out/renderer/api/apiServer.js

- Method (guessed): POST (data present)

- Sample line: 1471

- 参数示例未在函数中以静态对象形式发现；可能动态构造。

## /hcy/revision/restoreRevision

- Functions: getTypeListCache

- Files: out/renderer/api/apiServer.js

- Method (guessed): POST (data present)

- Sample line: 1511

- 参数示例未在函数中以静态对象形式发现；可能动态构造。

## /hcy/recyclebin/list

- Functions: if

- Files: out/renderer/api/apiServer.js

- Method (guessed): POST (data present)

- Sample line: 1562

- 参数示例未在函数中以静态对象形式发现；可能动态构造。

## /hcy/recyclebin/batchRestore

- Functions: if

- Files: out/renderer/api/apiServer.js

- Method (guessed): POST (data present)

- Sample line: 1580

- 参数示例未在函数中以静态对象形式发现；可能动态构造。

## /hcy/deleteContentsAsync

- Functions: if

- Files: out/renderer/api/apiServer.js

- Method (guessed): POST (data present)

- Sample line: 1602

- 参数示例未在函数中以静态对象形式发现；可能动态构造。

## /hcy/mount/task/get

- Functions: if

- Files: out/renderer/api/apiServer.js

- Method (guessed): POST (data present)

- Sample line: 1654

- 参数示例未在函数中以静态对象形式发现；可能动态构造。

## /hcy/file/update

- Functions: if, getAdvertisementInfo, getAdvertiseConfigInfo, pushNewAdInfos, clickNewAdInfos, getSpaceSize

- Files: out/renderer/api/apiServer.js

- Method (guessed): POST (data present)

- Sample line: 1682

- 参数示例未在函数中以静态对象形式发现；可能动态构造。

## /advertapi/adv-filter/adv-filter/AdInfoFilter/getAdInfos

- Functions: if, getFileAddress, getMountFileList

- Files: out/renderer/api/apiServer.js

- Method (guessed): POST (data present)

- Sample line: 1732

- 参数示例未在函数中以静态对象形式发现；可能动态构造。

## /advertapi/adv-config/adv-config/adFilter/getAdInfos

- Functions: if

- Files: out/renderer/api/apiServer.js

- Method (guessed): POST (data present)

- Sample line: 1756

- 参数示例未在函数中以静态对象形式发现；可能动态构造。

## /advertapi/adv-filter/adv-filter/advertReported/click

- Functions: if

- Files: out/renderer/api/apiServer.js

- Method (guessed): POST (data present)

- Sample line: 1775

- 参数示例未在函数中以静态对象形式发现；可能动态构造。

## /user/disk/getPersonalDiskInfo

- Functions: getVideoList

- Files: out/renderer/api/apiServer.js

- Method (guessed): POST (data present)

- Sample line: 1780

- 参数示例未在函数中以静态对象形式发现；可能动态构造。

## /hcy/file/getPath

- Functions: movieRename, movieDelete

- Files: out/renderer/api/apiServer.js

- Method (guessed): POST (data present)

- Sample line: 1790

- 参数示例未在函数中以静态对象形式发现；可能动态构造。

## /mount/file/statsDir

- Functions: queryMovieTask

- Files: out/renderer/api/apiServer.js

- Method (guessed): POST (data present)

- Sample line: 1800

- 参数示例未在函数中以静态对象形式发现；可能动态构造。

## /hcy/file/getDownloadUrl

- Functions: uploadPCStatus

- Files: out/renderer/api/apiServer.js

- Method (guessed): POST (data present)

- Sample line: 1816

- 参数示例未在函数中以静态对象形式发现；可能动态构造。

## /orchestration/file/video/v1.0/getVideoResourceList

- Functions: queryFileData

- Files: out/renderer/api/apiServer.js

- Method (guessed): POST (data present)

- Sample line: 1832

- 参数示例未在函数中以静态对象形式发现；可能动态构造。

## /orchestration/file/video/v1.0/updateVideoResource

- Functions: getDownloadUrl

- Files: out/renderer/api/apiServer.js

- Method (guessed): POST (data present)

- Sample line: 1840

- 参数示例未在函数中以静态对象形式发现；可能动态构造。

## /orchestration/file/task/v1.0/queryTaskDetail

- Functions: if

- Files: out/renderer/api/apiServer.js

- Method (guessed): POST (data present)

- Sample line: 1857

- 参数示例未在函数中以静态对象形式发现；可能动态构造。

## /openapi/interaction/uploadPCStatus

- Functions: getDownloadUrl

- Files: out/renderer/api/apiServer.js

- Method (guessed): POST (data present)

- Sample line: 1865

- 参数示例未在函数中以静态对象形式发现；可能动态构造。

## /openapi/interaction/queryFileData

- Functions: getFamilyList, queryCloudMember

- Files: out/renderer/api/apiServer.js

- Method (guessed): POST (data present)

- Sample line: 1870

- 参数示例未在函数中以静态对象形式发现；可能动态构造。

## /hcy/mutual/adapter/isbo/openApi/getGroupFileDownLoadURLV2

- Functions: getFamilyalbumList, getPhotoContents, createFamily

- Files: out/renderer/api/apiServer.js

- Method (guessed): POST (data present)

- Sample line: 1879

- 参数示例未在函数中以静态对象形式发现；可能动态构造。

## /hcy/group/dynamic/assets/getFlvOnlineAddr

- Functions: setFamilyCloudOnTV

- Files: out/renderer/api/apiServer.js

- Method (guessed): POST (data present)

- Sample line: 1893

- 参数示例未在函数中以静态对象形式发现；可能动态构造。

## /hcy/family/adapter/andAlbum/openApi/getFileDownLoadURLV2

- Functions: modifyFamilyNick, modifyCloudFamily, deleteFamilyCloud

- Files: out/renderer/api/apiServer.js

- Method (guessed): POST (data present)

- Sample line: 1898

- 参数示例未在函数中以静态对象形式发现；可能动态构造。

## /hcy/family/adapter/andAlbum/openApi/queryFamilyCloud

- Functions: quitFamilyCloud

- Files: out/renderer/api/apiServer.js

- Method (guessed): POST (data present)

- Sample line: 1913

- 参数示例未在函数中以静态对象形式发现；可能动态构造。

## /hcy/family/adapter/andAlbum/openApi/queryCloudMemberV3

- Functions: getWechatInviteQRCode

- Files: out/renderer/api/apiServer.js

- Method (guessed): POST (data present)

- Sample line: 1917

- 参数示例未在函数中以静态对象形式发现；可能动态构造。

## /hcy/family/adapter/andAlbum/openApi/queryContentInfoV2

- Functions: createInviteQRCode

- Files: out/renderer/api/apiServer.js

- Method (guessed): POST (data present)

- Sample line: 1931

- 参数示例未在函数中以静态对象形式发现；可能动态构造。

## /hcy/family/adapter/andAlbum/openApi/createFamilyCloud

- Functions: createCloudPhoto

- Files: out/renderer/api/apiServer.js

- Method (guessed): POST (data present)

- Sample line: 1935

- 参数示例未在函数中以静态对象形式发现；可能动态构造。

## /hcy/family/adapter/andAlbum/openApi/hideFamilyCloudOnTV

- Functions: deleteCloudMember

- Files: out/renderer/api/apiServer.js

- Method (guessed): POST (data present)

- Sample line: 1939

- 参数示例未在函数中以静态对象形式发现；可能动态构造。

## /hcy/family/adapter/andAlbum/openApi/modifyCloudMember

- Functions: queryContentListV3

- Files: out/renderer/api/apiServer.js

- Method (guessed): POST (data present)

- Sample line: 1944

- 参数示例未在函数中以静态对象形式发现；可能动态构造。

## /hcy/family/adapter/andAlbum/openApi/modifyFamilyCloud

- Functions: getCatalogInfos

- Files: out/renderer/api/apiServer.js

- Method (guessed): POST (data present)

- Sample line: 1948

- 参数示例未在函数中以静态对象形式发现；可能动态构造。

## /hcy/family/adapter/andAlbum/openApi/deleteFamilyCloud

- Functions: getUserInfo

- Files: out/renderer/api/apiServer.js

- Method (guessed): POST (data present)

- Sample line: 1952

- 参数示例未在函数中以静态对象形式发现；可能动态构造。

## /hcy/family/adapter/andAlbum/openApi/quitFamilyCloud

- Functions: modifyCloudPhoto

- Files: out/renderer/api/apiServer.js

- Method (guessed): POST (data present)

- Sample line: 1956

- 参数示例未在函数中以静态对象形式发现；可能动态构造。

## /hcy/family/adapter/andAlbum/openApi/wechatInvitation

- Functions: deleteCloudPhoto

- Files: out/renderer/api/apiServer.js

- Method (guessed): POST (data present)

- Sample line: 1960

- 参数示例未在函数中以静态对象形式发现；可能动态构造。

## /mcloud/wechat/api/wxapplet/getacode

- Functions: modifyContentInfo

- Files: out/renderer/api/apiServer.js

- Method (guessed): POST (data present)

- Sample line: 1965

- 参数示例未在函数中以静态对象形式发现；可能动态构造。

## /hcy/family/adapter/andAlbum/openApi/createCloudPhoto

- Functions: modifyCloudDocV2, createCloudDocV2

- Files: out/renderer/api/apiServer.js

- Method (guessed): POST (data present)

- Sample line: 1970

- 参数示例未在函数中以静态对象形式发现；可能动态构造。

## /hcy/family/adapter/andAlbum/openApi/deleteCloudMember

- Functions: createBatchOprTaskV2

- Files: out/renderer/api/apiServer.js

- Method (guessed): POST (data present)

- Sample line: 1981

- 参数示例未在函数中以静态对象形式发现；可能动态构造。

## /hcy/family/adapter/andAlbum/openApi/queryContentListV3

- Functions: queryBatchOprTaskV3

- Files: out/renderer/api/apiServer.js

- Method (guessed): POST (data present)

- Sample line: 1987

- 参数示例未在函数中以静态对象形式发现；可能动态构造。

## /hcy/family/adapter/andAlbum/openApi/getCatalogInfos

- Functions: noteInit, getNotebook

- Files: out/renderer/api/apiServer.js

- Method (guessed): POST (data present)

- Sample line: 1993

- 参数示例未在函数中以静态对象形式发现；可能动态构造。

## /hcy/family/adapter/andAlbum/openApi/getUserInfo

- Functions: getNoteDetail

- Files: out/renderer/api/apiServer.js

- Method (guessed): POST (data present)

- Sample line: 2001

- 参数示例未在函数中以静态对象形式发现；可能动态构造。

## /csbo/api/common/longTranslate

- Functions: getAiWhite

- Files: out/renderer/api/apiServer.js

- Method (guessed): POST (data present)

- Sample line: 2080

- 参数示例未在函数中以静态对象形式发现；可能动态构造。

## /csbo/api/common/getTranslateResult

- Functions: getMouseAuth, getCloudList

- Files: out/renderer/api/apiServer.js

- Method (guessed): POST (data present)

- Sample line: 2091

- 参数示例未在函数中以静态对象形式发现；可能动态构造。

## /yun-note/note/fileSaveToNote

- Functions: getCloudListInfo, clearCloudList

- Files: out/renderer/api/apiServer.js

- Method (guessed): POST (data present)

- Sample line: 2107

- 参数示例未在函数中以静态对象形式发现；可能动态构造。

## /openapi/general/getStatus

- Functions: clearContentList, getGroupList, getGroupFileList

- Files: out/renderer/api/apiServer.js

- Method (guessed): POST (data present)

- Sample line: 2121

- 参数示例未在函数中以静态对象形式发现；可能动态构造。

## /cloudSEE/openApi/queryRightsSubscribeRelation

- Functions: getGroupFileByType, getCloudList

- Files: out/renderer/api/apiServer.js

- Method (guessed): POST (data present)

- Sample line: 2138

- 参数示例未在函数中以静态对象形式发现；可能动态构造。

## /openapi/pDynamicInfo/queryBatchList

- Functions: getFamilyPhoto

- Files: out/renderer/api/apiServer.js

- Method (guessed): POST (data present)

- Sample line: 2148

- 参数示例未在函数中以静态对象形式发现；可能动态构造。

## /openapi/pDynamicInfo/queryBatchInfo

- Functions: getFamilyFile, getFamilyFileByPhoto

- Files: out/renderer/api/apiServer.js

- Method (guessed): POST (data present)

- Sample line: 2153

- 参数示例未在函数中以静态对象形式发现；可能动态构造。

## /openapi/pDynamicInfo/removeByInfoIds

- Functions: getFamilyFileByType

- Files: out/renderer/api/apiServer.js

- Method (guessed): POST (data present)

- Sample line: 2163

- 参数示例未在函数中以静态对象形式发现；可能动态构造。

## /openapi/pDynamicInfo/removeContents

- Functions: groupFileAction

- Files: out/renderer/api/apiServer.js

- Method (guessed): POST (data present)

- Sample line: 2170

- 参数示例未在函数中以静态对象形式发现；可能动态构造。

## /hcy/mutual/adapter/search/sharingSearch-ss001

- Functions: groupFileProgress

- Files: out/renderer/api/apiServer.js

- Method (guessed): POST (data present)

- Sample line: 2176

- 参数示例未在函数中以静态对象形式发现；可能动态构造。

## /hcy/mutual/adapter/search/sharingSearch-ss004

- Functions: if

- Files: out/renderer/api/apiServer.js

- Method (guessed): POST (data present)

- Sample line: 2191

- 参数示例未在函数中以静态对象形式发现；可能动态构造。

## /search/SearchFile

- Functions: getCommonCatalogList

- Files: out/renderer/api/apiServer.js

- Method (guessed): POST (data present)

- Sample line: 2196

- 参数示例未在函数中以静态对象形式发现；可能动态构造。

## /hcy/family/adapter/search/famSearch-fs001

- Functions: delPersonFile

- Files: out/renderer/api/apiServer.js

- Method (guessed): POST (data present)

- Sample line: 2204

- 参数示例未在函数中以静态对象形式发现；可能动态构造。

## /hcy/family/adapter/search/famSearch-fs004

- Functions: if

- Files: out/renderer/api/apiServer.js

- Method (guessed): POST (data present)

- Sample line: 2225

- 参数示例未在函数中以静态对象形式发现；可能动态构造。

## /hcy/mutual/adapter/isbo/openApi/createBatchOprTask

- Functions: copyPersonFile

- Files: out/renderer/api/apiServer.js

- Method (guessed): POST (data present)

- Sample line: 2230

- 参数示例未在函数中以静态对象形式发现；可能动态构造。

## /openapi/commonPath/getCommonPathByPhone

- Functions: if

- Files: out/renderer/api/apiServer.js

- Method (guessed): POST (data present)

- Sample line: 2252

- 参数示例未在函数中以静态对象形式发现；可能动态构造。

## /hcy/file/batchCopy

- Functions: if

- Files: out/renderer/api/apiServer.js

- Method (guessed): POST (data present)

- Sample line: 2282

- 参数示例未在函数中以静态对象形式发现；可能动态构造。

## /mount/archive/list

- Functions: if

- Files: out/renderer/api/apiServer.js

- Method (guessed): POST (data present)

- Sample line: 2304

- 参数示例未在函数中以静态对象形式发现；可能动态构造。

## /hcy/archive/getTask

- Functions: if

- Files: out/renderer/api/apiServer.js

- Method (guessed): POST (data present)

- Sample line: 2323

- 参数示例未在函数中以静态对象形式发现；可能动态构造。

## /hcy/archive/uncompress

- Functions: getFileInfo, convertFile

- Files: out/renderer/api/apiServer.js

- Method (guessed): POST (data present)

- Sample line: 2328

- 参数示例未在函数中以静态对象形式发现；可能动态构造。

## /hcy/archive/previewFiles

- Functions: convertPdf, convertProcess, queryMessageCenterInfo

- Files: out/renderer/api/apiServer.js

- Method (guessed): POST (data present)

- Sample line: 2338

- 参数示例未在函数中以静态对象形式发现；可能动态构造。

## /hcy/file/batchGet

- Functions: getShareList, cancelShare

- Files: out/renderer/api/apiServer.js

- Method (guessed): POST (data present)

- Sample line: 2362

- 参数示例未在函数中以静态对象形式发现；可能动态构造。

## /yun-share/richlifeApp/devapp/IOutLink

- Functions: if

- Files: out/renderer/api/apiServer.js

- Method (guessed): POST (data present)

- Sample line: 2422

- 参数示例未在函数中以静态对象形式发现；可能动态构造。

## /orchestration/thirdPart/wechat/v1.0/miniProgram/createQRCode

- Functions: getQRcode, queryControlSwitch, getRsaPublicKey, getSkinList, getGraySkin, get139mailUpgradeInfo, checkUpgrade

- Files: out/renderer/api/apiServer.js

- Method (guessed): POST (data present)

- Sample line: 2440

- 参数示例未在函数中以静态对象形式发现；可能动态构造。

## /yun-share/richlifeApp/devapp/IControl

- Functions: mountProgress, mountTransTaskProgress

- Files: out/renderer/api/apiServer.js

- Method (guessed): POST (data present)

- Sample line: 2502

- 参数示例未在函数中以静态对象形式发现；可能动态构造。

## /openapi/skinConfig/queryV2

- Functions: mountTrans, mountMove

- Files: out/renderer/api/apiServer.js

- Method (guessed): POST (data present)

- Sample line: 2519

- 参数示例未在函数中以静态对象形式发现；可能动态构造。

## /openapi/grayConfig/query

- Functions: mountRename, mountSearch

- Files: out/renderer/api/apiServer.js

- Method (guessed): POST (data present)

- Sample line: 2533

- 参数示例未在函数中以静态对象形式发现；可能动态构造。

## /user/version/check

- Functions: mountRecycleList

- Files: out/renderer/api/apiServer.js

- Method (guessed): POST (data present)

- Sample line: 2548


```javascript
{
        pageInfo: {
          pageCursor: item.nextPageCursor || "",
          pageSize: 20,
        },
      }
```


## /mount/task/get

- Functions: mountRestore

- Files: out/renderer/api/apiServer.js

- Method (guessed): POST (data present)

- Sample line: 2555


```javascript
{
        fileIds: item.fileIds,
      }
```


## /mount/transTask/get

- Functions: mountBatchDelete

- Files: out/renderer/api/apiServer.js

- Method (guessed): POST (data present)

- Sample line: 2563


```javascript
{
        fileIds: item.fileIds,
      }
```


## /mount/recyclebin/batchTrash

- Functions: mountHistory

- Files: out/renderer/api/apiServer.js

- Method (guessed): POST (data present)

- Sample line: 2567

- 参数示例未在函数中以静态对象形式发现；可能动态构造。

## /mount/file/search

- Functions: mountGetDownloadUrl

- Files: out/renderer/api/apiServer.js

- Method (guessed): POST (data present)

- Sample line: 2571

- 参数示例未在函数中以静态对象形式发现；可能动态构造。

## /mount/transTask/create

- Functions: mountDeleteHistoryList

- Files: out/renderer/api/apiServer.js

- Method (guessed): POST (data present)

- Sample line: 2575

- 参数示例未在函数中以静态对象形式发现；可能动态构造。

## /mount/file/batchMove

- Functions: mountRestoreRevision

- Files: out/renderer/api/apiServer.js

- Method (guessed): POST (data present)

- Sample line: 2579

- 参数示例未在函数中以静态对象形式发现；可能动态构造。

## /mount/file/update

- Functions: createKnowledgeBase

- Files: out/renderer/api/apiServer.js

- Method (guessed): POST (data present)

- Sample line: 2587

- 参数示例未在函数中以静态对象形式发现；可能动态构造。

## /search/SearchFileMountingDisc

- Functions: queryKnowledgeBaseList

- Files: out/renderer/api/apiServer.js

- Method (guessed): POST (data present)

- Sample line: 2592

- 参数示例未在函数中以静态对象形式发现；可能动态构造。

## /mount/recyclebin/list

- Functions: batchImport

- Files: out/renderer/api/apiServer.js

- Method (guessed): POST (data present)

- Sample line: 2597

- 参数示例未在函数中以静态对象形式发现；可能动态构造。

## /mount/recyclebin/batchRestore

- Functions: queryImportTaskList

- Files: out/renderer/api/apiServer.js

- Method (guessed): POST (data present)

- Sample line: 2602

- 参数示例未在函数中以静态对象形式发现；可能动态构造。

## /mount/file/batchDelete

- Functions: queryKnowledgeBaseResourceList

- Files: out/renderer/api/apiServer.js

- Method (guessed): POST (data present)

- Sample line: 2607

- 参数示例未在函数中以静态对象形式发现；可能动态构造。

## /mount/revision/listRevision

- Functions: onlineFile

- Files: out/renderer/api/apiServer.js

- Method (guessed): POST (data present)

- Sample line: 2616

- 参数示例未在函数中以静态对象形式发现；可能动态构造。

## /hcy/mutual/adapter/isbo/openApi/queryGroupV2

- Functions: getGroupList, addGroupList, getGroupFileList, getGroupMerberList, getGroupInfo, updateGroupInfo, disbandGroup, addNewFile, if

- Files: out/renderer/api/groupServer.js

- Method (guessed): POST (data present)

- Sample line: 8

- 参数示例未在函数中以静态对象形式发现；可能动态构造。

## /hcy/mutual/adapter/isbo/openApi/createGroupV2

- Functions: generateInvitationUrl

- Files: out/renderer/api/groupServer.js

- Method (guessed): POST (data present)

- Sample line: 60

- 参数示例未在函数中以静态对象形式发现；可能动态构造。

## /hcy/mutual/adapter/isbo/openApi/queryGroupContentListV2

- Functions: delGroupFile

- Files: out/renderer/api/groupServer.js

- Method (guessed): POST (data present)

- Sample line: 65

- 参数示例未在函数中以静态对象形式发现；可能动态构造。

## /hcy/mutual/adapter/isbo/openApi/queryMembersV2

- Functions: batchSetMemberRole, deletMembers

- Files: out/renderer/api/groupServer.js

- Method (guessed): POST (data present)

- Sample line: 70

- 参数示例未在函数中以静态对象形式发现；可能动态构造。

## /hcy/mutual/adapter/isbo/openApi/queryGroup

- Functions: updateMember

- Files: out/renderer/api/groupServer.js

- Method (guessed): POST (data present)

- Sample line: 80

- 参数示例未在函数中以静态对象形式发现；可能动态构造。