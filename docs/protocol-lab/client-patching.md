audience: external
# CN 客户端打补丁

本页适用于把合法取得的单个 CN APK 连接到本机 Starpoint CN 服务.

`patch-cn-client.ps1` 从 `assets/worldflipper_android_release.swf` 直接补丁 `pinball.gbits.logic.GbitsVersionLogic` 的两个版本地址, 再只导出和导入 `pinball.config.gbits.DevConfig_gf_android` 来修改 API 端点, 最后重建 APK. 脚本不会匹配任意 URL. 每个已知地址和默认 API 端点必须恰好出现 1 次, 否则脚本停止.

脚本只替换以下地址:

- `https://update.leiting.com/shijtswy/version/`
- `https://update.roguelike.com/shijtswy/version/`
- `ApiServerKind.Custom("http","10.0.2.2:8001")`
- `ApiServerKind.Custom("https","shijtswygamegf.leiting.com")`

## 前置条件

准备以下文件:

- 一个包含 `assets/worldflipper_android_release.swf` 的 CN APK.
- Starview Windows 工具目录. 默认位置是 workspace 的 `artifacts/starview-tools/starview-windows`.
- JDK 17. 脚本使用其中的 `java.exe` 和 `keytool.exe`. 版本地址补丁以及 `-ProbeSdkDummyLoginRequest`, `-ProbeVersionQuery`, `-ProbeMessagePackDecodePosition` 和 `-UsePreextractedBundle` 还使用 `javac.exe`.

Starview 工具目录必须包含 `ffdec/ffdec.jar`, `build-tools/zipalign.exe` 和 `build-tools/lib/apksigner.jar`. 版本地址补丁和三个诊断补丁还要求 `ffdec/lib/ffdec_lib.jar`.

## 运行

从仓库根目录运行:

```powershell
./scripts/protocol-lab/patch-cn-client.ps1 `
    -InputApkPath C:\path\to\worldflipper-cn.apk `
    -Host 10.0.2.2 `
    -Port 8001
```

`10.0.2.2` 是 Android Emulator 访问 Windows 宿主机的地址. 真机需要使用设备可访问的服务器 DNS 名称或 IP 地址.

正式管线先用 `GbitsVersionUrlAbcPatch` 直接修改 `GbitsVersionLogic.queryVersion()` 和 `onQueryErrorDefault()` 的版本地址, 再只让 FFDec 导出和导入 API 配置类. 版本类不会经过 ActionScript 重编译. 版本地址补丁保存 reference/input/output 方法摘要, 并拒绝除两个目标方法之外的变化. 协议实验可以额外使用 `-ProbeSdkDummyLoginRequest`. 该开关把唯一的 `sdkDummy` 声明设为 `true`, 让渠道入口使用 `ChannelSDKDummy`, 并让其 `startLoginServer()` 调用客户端原始的 `testLogin()` 请求. 探测模式额外导出和重编译 `DevConfig`, `TitleScene`, `ChannelSDKMain` 和 `ChannelSDKDummy`. FFDec 不导出或重编译 `RemoteUtil` 或 `GbitsVersionLogic`. FFDec 导入完成后, `RemoteUtilAbcPatch` 只把 `getURLRequest()` 中唯一的雷霆原生扩展初始化序列替换为 `false`, 并拒绝任何其他静态方法的签名, 方法体或引用常量语义变化. 未启用解码位置诊断时, `requestCompleteHandler()` 必须与原始 SWF 的语义摘要完全相同. 响应到达通过 `ChannelSDKDummy.loginSuccessHandler()` 和后续真实 HTTP 请求观察, 不再向不完整的反编译响应方法插入 `trace`. 该开关不替换 `testLogin()` 的 `channels/channel_leiting/leiting_login` 请求, 不伪造服务器响应, 也不设置 SDK 登录成功状态. 该探测只验证补丁后的静态结构, 不构成动态协议兼容性或登录成功验证. 不传该开关时, 这些 dummy SDK 类不会被导出, 选择或修改, `RemoteUtil` 也不会被修改. 探针还记录 `TitleScene.buttonClicked(0)` 的登录门控: 版本查询成功, SDK 初始化完成且未处于登录中时才进入 SDK 登录分支; `run()` 初始禁用按钮 0, 序列切换后才启用按钮 0. 该门控是静态证据, 不代表按钮输入已经在模拟器中触发.

`-ProbeVersionQuery` 必须和 `-ProbeSdkDummyLoginRequest` 一起使用. 诊断工具只修改 `GbitsVersionLogic.isQuerySuccess()` 的 AVM2 方法体, 删除 `DevConfig.sdkDummy` 的提前成功分支, 保留 `publishTarget` 和 `versions` 判断, 并在保存前后验证唯一方法摘要变化. 该模式使用版本地址补丁后的 SWF 作为输入, 用于避免登录请求探针把版本查询状态机短路. 它不伪造版本响应, 不声明版本请求动态成功, 并要求之后用安全 HTTP 访问日志捕获真实 `client_release_<target>_<platform>.dis` 请求.

`-ProbeMessagePackDecodePosition` 只用于定位真实响应解码错误, 并要求同时启用 `-ProbeSdkDummyLoginRequest`. `RemoteUtilDecodeDiagnosticPatch` 先验证原始目标方法没有被 FFDec 或登录探测改写, 再向 `requestCompleteHandler()` 的 ParseError 文本追加 Base64 文本长度, 完整摘要, 4096, 8192 和 12288 字符前缀的摘要前 8 字符, 解码字节摘要, Decoder 当前位置和剩余字节数. 同一 ABC 补丁只把 `DisplayableError.getDisplayMessage()` 的内部错误条件替换为 `true`, 让错误对话框显示该诊断文本. 该补丁不改变 Decoder, 成功回调, 结果码分支或服务器响应. 清单明确记录该 APK 未验证协议成功. 普通测试和交付 APK 不使用此开关.

`-UsePreextractedBundle` 只用于现代 Android 协议实验环境. `AssetExtractorPreextractedBundlePatch` 把 `AssetExtractor.start()` 中进入 `readBinaryFileAsync(bundle.zip)` 的唯一条件替换为 `false`, 并保留其他实例方法. 该模式要求设备应用存储中已经存在完整的 `asset/bundle/production` 和与 APK 一致的 `asset/bundle/bundle.zip.sha1`. 补丁不创建, 下载或验证这些文件, 缺少前置资源时不得启动客户端. 该模式只绕过 AIR 在当前模拟环境读取内置大 ZIP 的兼容性阻断, 不构成协议成功或资源完整性证据.

`start-cn-login-transport-server.mjs` 和 `start-cn-login-minimal-transport-server.mjs` 只用于响应头实验. 它们在 `CN_LEITING_LOGIN_HEADERS=transport` 或 `minimal-transport` 下增加 `x-result-code`, `Connection` 和可控 `param` 头. `param` 默认是 `probe-param`, 不是协议成功凭据, 不能据此判断客户端接受响应.

明确指定输出和工具位置:

```powershell
./scripts/protocol-lab/patch-cn-client.ps1 `
    -InputApkPath C:\path\to\worldflipper-cn.apk `
    -OutputApkPath C:\path\to\worldflipper-cn-patched.apk `
    -StarviewToolsRoot C:\tools\starview-windows `
    -JavaHome "C:\Program Files\Microsoft\jdk-17.0.10.7-hotspot" `
    -Host 192.168.1.20 `
    -Port 8001
```

默认工作目录是 workspace 的 `artifacts/protocol-lab/client-patching/<UTC 时间>`. 默认输出 APK 位于该工作目录. 输入 APK 保持只读.

## 本机回归测试

不带外部文件运行时, 测试验证 PowerShell 补丁选择和证据校验:

```powershell
./scripts/protocol-lab/test-cn-sdk-login-bridge.ps1
```

提供仓库外的 FFDec 库和原始 CN SWF 时, 测试还会编译 Java 工具, 并验证方法签名, 引用常量语义和未声明方法体的变更都会被拒绝:

```powershell
./scripts/protocol-lab/test-cn-sdk-login-bridge.ps1 `
    -FfdecLibraryJarPath C:\path\to\ffdec\lib\ffdec_lib.jar `
    -ReferenceSwfPath C:\path\to\worldflipper_android_release.swf
```

同一组 Java 工具还会验证解码诊断补丁只修改 `requestCompleteHandler` 和 `DisplayableError.getDisplayMessage`, 预展开资源补丁只修改 `AssetExtractor.start`, 并拒绝重复应用. 需要定位其他 AVM2 方法引用时, 使用 `AbcMethodReferenceSearch`:

```powershell
java -cp "<classes>;ffdec/lib/*" AbcMethodReferenceSearch `
    C:\path\to\worldflipper_android_release.swf RemoteUtil Decoder AssetExtractor
```

## 运行时签名

脚本默认在 workspace 的 `artifacts/protocol-lab/signing` 中生成以下文件:

- `cn-client-runtime.p12` 保存测试签名密钥.
- `cn-client-runtime.pass` 保存自动生成的随机口令.

后续运行复用这两个文件. 复用同一 keystore 可以更新已经安装的测试客户端. 删除或更换 keystore 后, Android 会把新 APK 视为不同签名, 需要先卸载旧客户端.

使用已有的非发行 keystore:

```powershell
./scripts/protocol-lab/patch-cn-client.ps1 `
    -InputApkPath C:\path\to\worldflipper-cn.apk `
    -KeystorePath C:\private\cn-client.p12 `
    -KeystorePasswordFile C:\private\cn-client.pass `
    -KeystoreType PKCS12 `
    -KeyAlias cn-client `
    -Host 10.0.2.2 `
    -Port 8001
```

口令文件只包含一行口令. 脚本拒绝把 keystore 或口令文件写入仓库目录. 不要把运行时签名用于发布, 不要提交 keystore 或口令文件.

## 证据和失败检查

成功后, `patch-manifest.json` 记录以下证据:

- 输入 APK, 输入 SWF, 修改后 SWF, 对齐 APK 和签名 APK 的 SHA-256.
- 输入和输出 `AndroidManifest.xml` 条目哈希. 两个哈希必须相同.
- 两个精确 URL 替换记录, 默认 API 端点替换记录和目标服务器地址.
- `-ProbeSdkDummyLoginRequest` 下的 dummy SDK 类名, 源文件前后哈希, 原始 `testLogin()` 保留记录, `TitleScene` 登录按钮门控证据, `RemoteUtil` Java 工具源文件哈希, 方法级 ABC 摘要和静态导入验证结果. 清单明确该探测没有执行动态验证, 也不验证服务器响应或 SDK 登录状态.
- `-ProbeVersionQuery` 下的 `GbitsVersionLogic.isQuerySuccess` 唯一方法摘要变化, `sdkDummy` 分支移除证据和保留的版本判断. 清单明确该探测不验证版本请求动态成功.
- `-ProbeMessagePackDecodePosition` 下的两个目标方法, 诊断字段, 输入和输出方法摘要. 清单明确该补丁只增加诊断信息, 不验证服务器响应或 load 成功.
- `-UsePreextractedBundle` 下的 `AssetExtractor.start` 方法摘要和预展开资源前置条件. 清单明确该补丁不验证资源内容或动态成功.
- FFDec, Android build-tools, apksigner 和 JDK 的版本, 路径和工具哈希.
- keystore 路径, alias 和证书证据路径. 清单不记录口令.

`evidence` 目录保存 FFDec 导出和导入日志, 五个方法级 ABC 工具的编译和方法摘要证据, `zipalign` 检查, `keytool` 证书信息和 `apksigner verify --print-certs` 输出.

脚本在以下情况停止并保留工作目录用于检查:

- 输入不是单个 APK, 或目标 SWF 条目不存在或重复.
- 目标类不存在或重复.
- 两个外部 URL 中任意一个不是恰好出现 1 次.
- 启用 `-ProbeSdkDummyLoginRequest` 时, `DevConfig`, `TitleScene`, `ChannelSDKMain` 或 `ChannelSDKDummy` 的目标类和唯一补丁结构无法定位, 重复出现, 或导入后验证失败.
- `RemoteUtil` 的目标类或原生扩展序列不唯一, FFDec 中间 SWF 已修改该类, 或除 `getURLRequest()` 外的静态方法摘要发生变化.
- 解码位置诊断没有找到唯一的 ParseError 构造序列, 输入响应方法已改变, 或输出还修改了其他方法.
- 预展开资源补丁没有找到 `AssetExtractor.start` 中唯一的内置 ZIP 分支条件, 输入方法已改变, 或输出还修改了其他实例方法.
- 启用 `-ProbeVersionQuery` 时, `GbitsVersionLogic.isQuerySuccess` 的 sdkDummy 分支不唯一, 版本判断缺失, 或输出修改了其他实例方法.
- FFDec 导入后再次导出时没有得到恰好 2 个目标地址.
- 重建后的 SWF 哈希或 `AndroidManifest.xml` 哈希不匹配.
- `zipalign` 或 APK 签名验证失败.

该流程验证补丁和签名, 不验证服务器协议兼容性. 安装后仍需启动 CN 服务并运行客户端连接测试.
