<!-- audience: external -->
# Starpoint Mobile

本说明适用于公开仓库的源码获取, CN CDN 对齐, 平台构建和资源替换.

Starpoint Mobile 把本地个人服务, 管理页面, CN 客户端兼容逻辑和资源打包流程组合为 iOS 与 Android 工程. 两个平台共享 `core/personal-service`, 活动, 商店, 卡池, 战斗, 邮件, 虚拟时间和便携存档使用同一套服务实现.

## 分支

| 分支 | 内容 |
| --- | --- |
| [`main`](https://github.com/x380kkm/starpoint-public/tree/main) | 项目说明和资源准备入口. |
| [`ios`](https://github.com/x380kkm/starpoint-public/tree/ios) | iOS Bootstrap, Framework, 客户端处理, macOS 构建和签名脚本. |
| [`android`](https://github.com/x380kkm/starpoint-public/tree/android) | Android CompanionHost, JNI, 客户端处理, CDN 合包和无头 AVD 工具. |

每个分支是一条独立的单提交历史. 选择目标平台后直接克隆对应分支:

```powershell
git clone --branch ios --single-branch https://github.com/x380kkm/starpoint-public.git starpoint-ios
git clone --branch android --single-branch https://github.com/x380kkm/starpoint-public.git starpoint-android
```

也可以先获取说明分支, 再切换到平台分支:

```powershell
git clone https://github.com/x380kkm/starpoint-public.git starpoint
Set-Location starpoint
git switch ios
```

## 外部输入

仓库提供源码, 可再生成的数据, 确定性资源清单和处理脚本. 使用者在仓库外准备以下输入:

- 有权使用的 CN 客户端. iOS 流程使用 CN 1.8.4 IPA, Android 流程使用对应 CN APK.
- CN CDN 基线. 项目内 `deployment/cn/cdn-manifest.json` 固定资源版本, Release 分片, 大小和 SHA-256.
- 平台签名材料. iOS 使用 Apple Development 证书和 provisioning profile; Android 使用发布者自己的 keystore.
- 可选的 JP 或 EN 区域资源. 这些输入用于补齐 CN 缺失语音或区域图片, 处理脚本保留 CN 文本和逻辑路径.

游戏客户端, 完整 CDN, 私钥, provisioning profile, keystore, 玩家存档和设备日志由使用者保存在仓库外.

## 准备 CN CDN

构建脚本统一读取仓库根目录下的 `.cdn/cn`. 一个可用的 CDN 根具有以下入口之一:

```text
.cdn/cn/path
.cdn/cn/entities/PathFile.csv
```

或:

```text
.cdn/cn/path
.cdn/cn/EntityLists/
```

### 从固定清单获取基线

平台分支携带 `deployment/cn/prepare-cdn.mjs`. 它从 `deployment/cn/cdn-manifest.json` 指定的 GitHub Release 下载分片, 校验大小与 SHA-256, 再流式解包到 `.cdn/cn`:

```powershell
node deployment/cn/prepare-cdn.mjs --cdn-dir .cdn
```

需要 Node.js 20.6 或更高版本, Python 3.12, PowerShell 7, GitHub CLI 和 `tar`.

### 对齐已有 CDN

`scripts/public/prepare_cdn.py` 把已有完整 CDN 复制到项目约定位置. 此方式适合从另一台机器, 已解包归档或现有工作目录恢复开发环境:

```powershell
python scripts/public/prepare_cdn.py --source D:\resources\cn
```

目标默认是 `.cdn/cn`. 可以显式指定其他位置:

```powershell
python scripts/public/prepare_cdn.py `
  --source D:\resources\cn `
  --destination D:\starpoint-data\cn
```

脚本保持全部相对路径, 拒绝链接越界, 路径越界和大小写冲突, 并写入 `.starpoint-cdn-layout.json`. 布局记录只保存输入显示名, 文件数和字节数.

### 叠加资源替换

覆盖目录使用与 CDN 根相同的相对路径. `--overlay` 可以重复提供, 后面的覆盖层替换前面同路径文件:

```powershell
python scripts/public/prepare_cdn.py `
  --source D:\resources\cn `
  --overlay D:\resources\voice-overlay `
  --overlay D:\resources\banner-overlay
```

默认同步会保留目标目录中额外存在的文件, 便于反复追加小型修复. 需要生成与本次输入完全一致的目录时使用 `--prune`:

```powershell
python scripts/public/prepare_cdn.py --source D:\resources\cn --prune
```

完成后确认以下命令能够读取资源入口:

```powershell
Get-Item .cdn\cn\path
Get-ChildItem .cdn\cn\entities\PathFile.csv, .cdn\cn\EntityLists -ErrorAction SilentlyContinue
```

## 资源处理

平台分支中的资源生成器读取 CN master, orderedmap, EntityLists 和允许使用的区域资源, 再输出可叠加到 CDN 的目录:

- `scripts/protocol-lab/generate-cn-activity-catalog.mjs` 生成活动目录.
- `scripts/protocol-lab/generate-cn-shop-contracts.mjs` 生成商店契约.
- `scripts/protocol-lab/generate_cn_gacha_banners.py` 解析卡池 banner, 并从同池角色或装备图标生成缺失图片.
- `scripts/protocol-lab/build-cn-voice-overlay.py` 为 CN 缺失语音生成区域资源覆盖层.
- `scripts/protocol-lab/cn_voice_overlay_archive.py` 把语音差分, EntityLists 和 `path` 更新合入 CDN.

生成结果作为 `prepare_cdn.py --overlay` 的输入. 同一逻辑路径只保留最终希望进入包体的文件.

## 服务开发

Node.js 服务和管理页面:

```powershell
npm ci
npm run build
```

嵌入式个人服务:

```powershell
cargo build --manifest-path core/personal-service/Cargo.toml
```

运行时数据库, 管理状态和构建产物分别写入 `.database`, `.management` 和工作区级 `artifacts`.

## iOS 构建

iOS Framework 在 macOS 上编译. Windows 侧候选构建脚本通过 SSH 使用用户配置的 macOS 主机, 将 Framework 回传后与 IPA 和 `.cdn/cn` 合包:

```powershell
pwsh scripts/protocol-lab/build-ios-cn-candidate.ps1 `
  -InputIpa D:\inputs\starpoint-cn-1.8.4.ipa `
  -CnCdnBundle .cdn\cn `
  -BundleId dev.example.starpoint `
  -DisplayName Starpoint `
  -SshHost my-mac
```

候选 IPA 是 unsigned 产物. 安装者在 macOS 上使用 `platforms/ios/sign-device-ipa.sh` 和自己的证书及 provisioning profile 完成签名.

## Android 构建

Android 完整分发脚本组合客户端补丁, ARM64 Rust 核心, JNI, CompanionHost 和 `.cdn/cn`:

```powershell
pwsh scripts/protocol-lab/build-android-cn-distribution.ps1 `
  -InputApkPath D:\inputs\starpoint-cn.apk `
  -OutputDirectory D:\outputs\starpoint-android `
  -SourceCdnRoot .cdn\cn `
  -KeystorePath D:\signing\release.keystore `
  -KeystorePasswordFile D:\signing\release-password.txt
```

Android 客户端验证使用项目创建的专用无头 AVD. ADB 调用由项目脚本绑定到该 AVD 的 serial, 使验证环境与其他窗口和设备隔离:

```powershell
pwsh scripts/protocol-lab/setup-emulator.ps1
pwsh scripts/protocol-lab/start-emulator.ps1
pwsh scripts/protocol-lab/verify-emulator.ps1
```

## 参考来源

- [duosii/starpoint](https://github.com/duosii/starpoint): Global 服务实现和路由行为.
- [duosii/starview](https://github.com/duosii/starview): 客户端资源, SWF 和 ActionScript 检查工具.
- [dennis96292/startpoint-cn-launcher](https://github.com/dennis96292/startpoint-cn-launcher): CN 客户端补丁和本地服务行为.
- [dennis96292/.cdn](https://github.com/dennis96292/.cdn): `deployment/cn/cdn-manifest.json` 固定的 CN CDN Release 来源.

使用客户端和区域资源时应遵守其许可条款及所在地法律. 本项目与游戏发行商及上述参考项目保持独立.
