# audience: internal
# # test-cn-sdk-login-bridge
# 此脚本在临时夹具和真实 SWF 上验证登录, 版本查询, 响应解码和预展开资源补丁只修改声明过的方法.

[CmdletBinding()]
param(
    [string]$FfdecLibraryJarPath,
    [string]$ReferenceSwfPath
)

Set-StrictMode -Version Latest
$ErrorActionPreference = "Stop"

# //// 导入补丁脚本中的测试函数 [@x380kkm 2026-07-28] ////
function Import-TestPatchFunctions {
    [CmdletBinding()]
    param([Parameter(Mandatory)][string]$PatchScriptPath)

    $Tokens = $null
    $ParseErrors = $null
    $Ast = [Management.Automation.Language.Parser]::ParseFile($PatchScriptPath, [ref]$Tokens, [ref]$ParseErrors)
    if ($ParseErrors.Count -ne 0) {
        throw "补丁脚本存在 PowerShell 语法错误: $($ParseErrors[0].Message)"
    }

    $FunctionNames = @(
        "Get-ClientPatchSdkDummyLoginRequestProbeMode",
        "Get-ClientPatchMessagePackDecodeDiagnosticMode",
        "Get-ClientPatchPreextractedBundleMode",
        "Get-ClientPatchVersionQueryProbeMode",
        "Test-ClientPatchAndroidServerHost",
        "Get-ClientPatchSelectedClassNames",
        "Set-ClientPatchSdkDummy",
        "Get-ClientPatchSdkDummyNativeExtensionMethods",
        "Set-ClientPatchSdkDummyNativeExtensionGuard",
        "Assert-ClientPatchSdkDummyNativeExtensionGuard",
        "Assert-ClientPatchRemoteUtilAbcEvidence",
        "Assert-ClientPatchRemoteUtilDecodeDiagnosticEvidence",
        "Assert-ClientPatchAssetExtractorPreextractedEvidence",
        "Assert-ClientPatchVersionQueryProbeEvidence",
        "Assert-ClientPatchVersionUrlAbcEvidence",
        "Get-ClientPatchOrdinalCount",
        "Get-ClientPatchSdkDummyTitleSceneMethods",
        "Set-ClientPatchSdkDummyTitleSceneGuard",
        "Assert-ClientPatchSdkDummyTitleSceneGuard",
        "Get-ClientPatchTitleSceneLoginGateEvidence",
        "Assert-ClientPatchTitleSceneLoginGateEvidence",
        "Set-ClientPatchSdkDummyRealRemoteBridge",
        "Get-ClientPatchSdkDummyTestLoginMethodContent",
        "Set-ClientPatchSdkDummyLoginRequestProbe",
        "Assert-ClientPatchSdkDummyLoginRequestProbe"
    )
    foreach ($FunctionName in $FunctionNames) {
        $Definitions = @($Ast.FindAll({ param($Node) $Node -is [Management.Automation.Language.FunctionDefinitionAst] -and $Node.Name -ceq $FunctionName }, $true))
        if ($Definitions.Count -ne 1) {
            throw "补丁函数数量不正确: function=$FunctionName actual=$($Definitions.Count)"
        }
        $Definitions[0].Extent.Text
    }
}
# //// /导入补丁脚本中的测试函数 ////

# //// 运行客户端 ABC 工具真实 SWF 回归测试 [@x380kkm 2026-08-11] ////
function Invoke-TestClientAbcPatches {
    [CmdletBinding()]
    param(
        [Parameter(Mandatory)][string]$PatchSourcePath,
        [Parameter(Mandatory)][string]$AbcDigestSourcePath,
        [Parameter(Mandatory)][string]$DigestSourcePath,
        [Parameter(Mandatory)][string]$TestSourcePath,
        [Parameter(Mandatory)][string]$DecodePatchSourcePath,
        [Parameter(Mandatory)][string]$DecodeTestSourcePath,
        [Parameter(Mandatory)][string]$AssetPatchSourcePath,
        [Parameter(Mandatory)][string]$AssetTestSourcePath,
        [Parameter(Mandatory)][string]$VersionPatchSourcePath,
        [Parameter(Mandatory)][string]$VersionTestSourcePath,
        [Parameter(Mandatory)][string]$VersionUrlPatchSourcePath,
        [Parameter(Mandatory)][string]$VersionUrlTestSourcePath,
        [string]$LibraryJarPath,
        [string]$SwfPath,
        [Parameter(Mandatory)][string]$TemporaryDirectory
    )

    if ([string]::IsNullOrWhiteSpace($LibraryJarPath) -and [string]::IsNullOrWhiteSpace($SwfPath)) {
        return [pscustomobject]@{ Performed = $false }
    }
    if ([string]::IsNullOrWhiteSpace($LibraryJarPath) -or [string]::IsNullOrWhiteSpace($SwfPath)) {
        throw "RemoteUtil 真实 SWF 测试必须同时提供 FFDec 库和参考 SWF"
    }
    foreach ($RequiredFile in @($PatchSourcePath, $AbcDigestSourcePath, $DigestSourcePath, $TestSourcePath, $DecodePatchSourcePath, $DecodeTestSourcePath, $AssetPatchSourcePath, $AssetTestSourcePath, $VersionPatchSourcePath, $VersionTestSourcePath, $VersionUrlPatchSourcePath, $VersionUrlTestSourcePath, $LibraryJarPath, $SwfPath)) {
        if (-not (Test-Path -LiteralPath $RequiredFile -PathType Leaf)) {
            throw "RemoteUtil 真实 SWF 测试文件不存在: $RequiredFile"
        }
    }

    $JavacPath = (Get-Command javac -ErrorAction Stop).Source
    $JavaPath = (Get-Command java -ErrorAction Stop).Source
    $ClassesDirectory = Join-Path $TemporaryDirectory "remoteutil-java-classes"
    $MutationDirectory = Join-Path $TemporaryDirectory "remoteutil-java-mutations"
    New-Item -ItemType Directory -Path $ClassesDirectory, $MutationDirectory | Out-Null

    $CompileOutput = @(& $JavacPath -encoding UTF-8 -cp $LibraryJarPath -d $ClassesDirectory $PatchSourcePath $AbcDigestSourcePath $DigestSourcePath $TestSourcePath $DecodePatchSourcePath $DecodeTestSourcePath $AssetPatchSourcePath $AssetTestSourcePath $VersionPatchSourcePath $VersionTestSourcePath $VersionUrlPatchSourcePath $VersionUrlTestSourcePath 2>&1)
    if ($LASTEXITCODE -ne 0) {
        throw "RemoteUtil Java 回归测试编译失败: exit=$LASTEXITCODE output=$($CompileOutput -join ' | ')"
    }

    $LibraryDirectory = Split-Path -Parent $LibraryJarPath
    $RuntimeClassPath = "$ClassesDirectory;$LibraryDirectory\*"
    $TestOutput = @(& $JavaPath -cp $RuntimeClassPath RemoteUtilAbcPatchTest $SwfPath $MutationDirectory 2>&1)
    if ($LASTEXITCODE -ne 0) {
        throw "RemoteUtil Java 回归测试失败: exit=$LASTEXITCODE output=$($TestOutput -join ' | ')"
    }

    $DecodeMutationDirectory = Join-Path $MutationDirectory "decode-diagnostic"
    New-Item -ItemType Directory -Path $DecodeMutationDirectory | Out-Null
    $DecodeTestOutput = @(& $JavaPath -cp $RuntimeClassPath RemoteUtilDecodeDiagnosticPatchTest $SwfPath $DecodeMutationDirectory 2>&1)
    if ($LASTEXITCODE -ne 0) {
        throw "RemoteUtil 解码诊断 Java 回归测试失败: exit=$LASTEXITCODE output=$($DecodeTestOutput -join ' | ')"
    }

    $AssetMutationDirectory = Join-Path $MutationDirectory "asset-extractor-preextracted-bundle"
    New-Item -ItemType Directory -Path $AssetMutationDirectory | Out-Null
    $AssetTestOutput = @(& $JavaPath -cp $RuntimeClassPath AssetExtractorPreextractedBundlePatchTest $SwfPath $AssetMutationDirectory 2>&1)
    if ($LASTEXITCODE -ne 0) {
        throw "AssetExtractor 预展开资源 Java 回归测试失败: exit=$LASTEXITCODE output=$($AssetTestOutput -join ' | ')"
    }

    $VersionMutationDirectory = Join-Path $MutationDirectory "gbits-version-query"
    New-Item -ItemType Directory -Path $VersionMutationDirectory | Out-Null
    $VersionTestOutput = @(& $JavaPath -cp $RuntimeClassPath GbitsVersionQueryProbePatchTest $SwfPath $VersionMutationDirectory 2>&1)
    if ($LASTEXITCODE -ne 0) {
        throw "GbitsVersionLogic 版本查询 Java 回归测试失败: exit=$LASTEXITCODE output=$($VersionTestOutput -join ' | ')"
    }

    $VersionUrlMutationDirectory = Join-Path $MutationDirectory "gbits-version-url"
    New-Item -ItemType Directory -Path $VersionUrlMutationDirectory | Out-Null
    $VersionUrlTestOutput = @(& $JavaPath -cp $RuntimeClassPath GbitsVersionUrlAbcPatchTest $SwfPath $VersionUrlMutationDirectory 2>&1)
    if ($LASTEXITCODE -ne 0) {
        throw "GbitsVersionLogic 版本地址 Java 回归测试失败: exit=$LASTEXITCODE output=$($VersionUrlTestOutput -join ' | ')"
    }

    [pscustomobject]@{
        Performed = $true
        Output = @($TestOutput) + @($DecodeTestOutput) + @($AssetTestOutput) + @($VersionTestOutput) + @($VersionUrlTestOutput)
    }
}
# //// /运行客户端 ABC 工具真实 SWF 回归测试 ////

# //// 写入渠道 dummy 登录请求 ActionScript 夹具 [@x380kkm 2026-07-28] ////
function Set-TestChannelSdkDummyFixture {
    [CmdletBinding()]
    param([Parameter(Mandatory)][string]$ScriptPath)

    $Content = @'
public class ChannelSDKDummy
{
   public var testConnect:Boolean;
    public var completeHandler:Function;
    public var lastOperationTime:Number;
    public var remote:Remote;
    public var titleScene:TitleScene;

   public function testLogin() : void
   {
      var request:Request = new Request("channels/channel_leiting/leiting_login");
      remote.requestQueue.addRequest(request);
   }

   public function loginSuccessHandler(param1:ResponseData) : void
   {
      var value:Object = param1.rawData.data;
   }

   public function startLoginServer(param1:Function) : void
   {
      nativeStartLoginServer();
   }
}
'@
    [IO.File]::WriteAllText($ScriptPath, $Content, [Text.UTF8Encoding]::new($false))
}
# //// /写入渠道 dummy 登录请求 ActionScript 夹具 ////

# //// 写入 sdkDummy 原生扩展入口 ActionScript 夹具 [@x380kkm 2026-07-28] ////
function Set-TestDevConfigNativeExtensionFixture {
    [CmdletBinding()]
    param([Parameter(Mandatory)][string]$ScriptPath)

    $Content = @'
public class DevConfig
{
   public static var sdkDummy:Boolean = false;
   public static var dummyChannel:String = "none";
   public static var dummyMedia:String = "none";

   public static function getRealChannel() : String
   {
      var extension:LeitingSDKExtension = LeitingSDKExtension.getInstance();
      var value:Object = extension.getPropertiesValue("channelType");
      if(DevConfig.dummyChannel == "none")
      {
         return value;
      }
      return DevConfig.dummyChannel;
   }

   public static function getRealMedia() : String
   {
      var extension:LeitingSDKExtension = LeitingSDKExtension.getInstance();
      var value:Object = extension.getPropertiesValue("media");
      if(DevConfig.dummyMedia == "none")
      {
         return value;
      }
      return DevConfig.dummyMedia;
   }

   public function isStrictRemote() : Boolean
   {
      return false;
   }
}
'@
    [IO.File]::WriteAllText($ScriptPath, $Content, [Text.UTF8Encoding]::new($false))
}
# //// /写入 sdkDummy 原生扩展入口 ActionScript 夹具 ////

# //// 写入渠道入口实例选择 ActionScript 夹具 [@x380kkm 2026-07-28] ////
function Set-TestChannelMainFixture {
    [CmdletBinding()]
    param([Parameter(Mandatory)][string]$ScriptPath)

    $Content = @'
package pinball.channels
{
   public class ChannelSDKMain
   {
      public function ChannelSDKMain(param1:RemoteKind, param2:RealRemote, param3:Logic)
      {
         if(param1 == RemoteKind.RealRemote)
         {
            realRemote = param2;
         }
         else
         {
            realRemote = null;
         }
         if(DevConfig.sdkDummy)
         {
            realRemote = null;
         }
      }

      public function init(param1:String, param2:TitleScene, param3:Function) : void
      {
         if(null == sdk)
         {
            if(null == realRemote)
            {
               sdk = new ChannelSDKDummy(realRemote,logic);
            }
            else
            {
               sdk = new ChannelLeitingSDKAndroid(realRemote,logic);
            }
         }
      }
   }
}
'@
    [IO.File]::WriteAllText($ScriptPath, $Content, [Text.UTF8Encoding]::new($false))
}
# //// /写入渠道入口实例选择 ActionScript 夹具 ////

# //// 写入标题场景原生微社区 ActionScript 夹具 [@x380kkm 2026-07-29] ////
function Set-TestTitleSceneFixture {
    [CmdletBinding()]
    param([Parameter(Mandatory)][string]$ScriptPath)

    $Content = @'
package pinball.scene.title
{
   public class TitleScene
   {
      public function openMicroCommunity(param1:String) : void
      {
         var _loc2_:String = JSON.stringify({"source":param1});
         LeitingSDKExtension.getInstance().showMicroCommunity(_loc2_);
      }

      public function initGbits() : void
      {
         channelSDK.init("leiting",this,sdkInit);
         if(channelSDK.sdkIsInited())
         {
            queryRemoteSign();
         }
         var _loc1_:LeitingSDKExtension = LeitingSDKExtension.getInstance();
         _loc1_.addEventListener(CallBack.MICROCOMMUNITYCALLBACK,onLeitingSDKStartCallBack);
         if(channelSDK.isSDKLoginOk())
         {
            openMicroCommunity("2");
         }
      }

      public function run() : void
      {
         buttonGroup = new ButtonGroupLogic(buttonClicked,_loc1_);
         buttonGroup.setEnabled(0,false);
      }

      public function onSequenceChangedTo(param1:String) : void
      {
         buttonGroup.setEnabled(0,true);
      }

      public function buttonClicked(param1:int) : void
      {
         switch(param1)
         {
            case 0:
               if(!versionLogic.isQuerySuccess())
               {
                  return;
               }
               if(!channelSDK.sdkIsInited())
               {
                  return;
               }
               if(channelSDK.isSDKLogining())
               {
                  return;
               }
               if(channelSDK.isSDKLoginOk())
               {
                  channelSDK.startLoginServer(onLoginServerSuccess);
               }
               else
               {
                  channelSDK.sdkLoginManual(onSDKLoginCompleteHander);
               }
               break;
         }
      }
   }
}
'@
    [IO.File]::WriteAllText($ScriptPath, $Content, [Text.UTF8Encoding]::new($false))
}
# //// /写入标题场景原生微社区 ActionScript 夹具 ////

# //// 断言一个条件成立 [@x380kkm 2026-07-28] ////
function Assert-TestCondition {
    [CmdletBinding()]
    param(
        [Parameter(Mandatory)][bool]$Condition,
        [Parameter(Mandatory)][string]$Message
    )

    if (-not $Condition) {
        throw $Message
    }
}
# //// /断言一个条件成立 ////

# //// 验证 SDK 登录请求探测模式 [@x380kkm 2026-07-29] ////
$PatchScriptPath = Join-Path $PSScriptRoot "patch-cn-client.ps1"
foreach ($FunctionSource in @(Import-TestPatchFunctions -PatchScriptPath $PatchScriptPath)) {
    Invoke-Expression $FunctionSource
}

$TemporaryRoot = [IO.Path]::GetFullPath([IO.Path]::GetTempPath())
$TemporaryDirectory = [IO.Path]::GetFullPath((Join-Path $TemporaryRoot ("starpoint-sdk-login-bridge-" + [Guid]::NewGuid().ToString("N"))))
New-Item -ItemType Directory -Path $TemporaryDirectory | Out-Null
try {
    $ProbeMode = Get-ClientPatchSdkDummyLoginRequestProbeMode -ProbeSdkDummyLoginRequest $true
    Assert-TestCondition -Condition ($ProbeMode.Name -ceq "dummy-login-request-probe") -Message "dummy 登录请求探测模式名称不正确"
    Assert-TestCondition -Condition ($ProbeMode.PatchSdkDummy -and $ProbeMode.PatchChannelMainForSdkDummy -and $ProbeMode.PatchDummyLoginRequestProbe -and $ProbeMode.PatchSdkDummyNativeExtensionGuard -and $ProbeMode.PatchRemoteUtilRequestGuard -and $ProbeMode.PatchSdkDummyTitleSceneGuard) -Message "dummy 登录请求探测模式没有启用原始请求路径和方法级守卫"

    Assert-TestCondition -Condition (-not (Test-ClientPatchAndroidServerHost -ServerHost "127.0.0.1") -and -not (Test-ClientPatchAndroidServerHost -ServerHost "localhost") -and -not (Test-ClientPatchAndroidServerHost -ServerHost "::1") -and (Test-ClientPatchAndroidServerHost -ServerHost "10.0.2.2") -and (Test-ClientPatchAndroidServerHost -ServerHost "192.168.1.20")) -Message "Android 模拟器目标地址守卫没有拒绝回环地址或接受可达地址"

    $DevConfigPath = Join-Path $TemporaryDirectory "dev-config.as"
    [IO.File]::WriteAllText($DevConfigPath, "public static var sdkDummy:Boolean = false;`n", [Text.UTF8Encoding]::new($false))
    $SdkDummyResult = Set-ClientPatchSdkDummy -ScriptPath $DevConfigPath
    Assert-TestCondition -Condition ($SdkDummyResult.Changed -and $SdkDummyResult.To -ceq "true" -and [IO.File]::ReadAllText($DevConfigPath).Contains("sdkDummy:Boolean = true", [StringComparison]::Ordinal)) -Message "登录请求探测没有启用 sdkDummy"

    $NativeExtensionPath = Join-Path $TemporaryDirectory "dev-config-native-extension.as"
    Set-TestDevConfigNativeExtensionFixture -ScriptPath $NativeExtensionPath
    Set-ClientPatchSdkDummy -ScriptPath $NativeExtensionPath | Out-Null
    $NativeExtensionResult = Set-ClientPatchSdkDummyNativeExtensionGuard -ScriptPath $NativeExtensionPath
    $NativeExtensionAfter = [IO.File]::ReadAllText($NativeExtensionPath)
    $NativeExtensionMethods = Get-ClientPatchSdkDummyNativeExtensionMethods -Content $NativeExtensionAfter
    Assert-TestCondition -Condition ($NativeExtensionResult.NativeExtensionCallsGuarded -and $NativeExtensionResult.MethodReplacements.Count -eq 2) -Message "sdkDummy 原生扩展守卫没有替换渠道和媒体方法"
    foreach ($MethodName in @('Channel', 'Media')) {
        $MethodContent = $NativeExtensionMethods.$MethodName
        $DummyValue = if ($MethodName -ceq 'Channel') { 'dummyChannel' } else { 'dummyMedia' }
        $GuardIndex = $MethodContent.IndexOf('if(DevConfig.sdkDummy)', [StringComparison]::Ordinal)
        $NativeIndex = $MethodContent.IndexOf('LeitingSDKExtension.getInstance()', [StringComparison]::Ordinal)
        Assert-TestCondition -Condition ($GuardIndex -ge 0 -and $NativeIndex -gt $GuardIndex -and $MethodContent.Substring(0, $NativeIndex).Contains("return DevConfig.$DummyValue;", [StringComparison]::Ordinal)) -Message "sdkDummy 原生扩展守卫没有在原生调用前返回占位值: method=$MethodName"
    }
    Assert-ClientPatchSdkDummyNativeExtensionGuard -ScriptPath $NativeExtensionPath

    $RemoteUtilEvidence = [pscustomobject]@{
        className = "pinball.context.remote.RemoteUtil"
        patchMethod = "getURLRequest"
        digestVersion = 2
        digestIncludes = @("method-info", "method-body", "constant-semantics", "exceptions", "traits")
        methodOnly = $true
        unchangedMethodsVerified = $true
        changedMethods = @("getURLRequest")
        nativeExtensionSequenceCountBefore = 1
        nativeExtensionSequenceCountAfter = 0
        requestCompleteHandler = [pscustomobject]@{
            referenceSha256 = "response-sha256"
            inputSha256 = "response-sha256"
            outputSha256 = "response-sha256"
        }
        getURLRequest = [pscustomobject]@{
            inputSha256 = "request-before-sha256"
            outputSha256 = "request-after-sha256"
        }
    }
    $RemoteUtilVerification = Assert-ClientPatchRemoteUtilAbcEvidence -Evidence $RemoteUtilEvidence
    Assert-TestCondition -Condition ($RemoteUtilVerification.Verified -and $RemoteUtilVerification.DigestVersion -eq 2 -and ($RemoteUtilVerification.DigestIncludes -join "|") -ceq ($RemoteUtilEvidence.digestIncludes -join "|") -and $RemoteUtilVerification.ChangedMethods.Count -eq 1 -and $RemoteUtilVerification.ChangedMethods[0] -ceq "getURLRequest") -Message "RemoteUtil 方法级补丁证据没有覆盖方法签名和常量语义或限制唯一修改方法"

    $DecodeEvidence = [pscustomobject]@{
        className = "pinball.context.remote.RemoteUtil"
        patchMethod = "requestCompleteHandler"
        digestVersion = 2
        methodOnly = $true
        changesErrorTextOnly = $true
        forcesInternalErrorDisplay = $true
        displayConditionIndex = 12
        diagnosticFields = @(
            "responseTextLength",
            "responseTextSha256",
            "responseTextPrefix4096Sha256Prefix",
            "responseTextPrefix8192Sha256Prefix",
            "responseTextPrefix12288Sha256Prefix",
            "decodedBytesLength",
            "decodedBytesSha256",
            "decoderPosition",
            "decoderBytesAvailable"
        )
        inputChangesFromReference = @("getURLRequest")
        changedMethods = @("requestCompleteHandler")
        requestCompleteHandler = [pscustomobject]@{
            referenceSha256 = "response-sha256"
            inputSha256 = "response-sha256"
            outputSha256 = "diagnostic-response-sha256"
        }
        getURLRequest = [pscustomobject]@{
            inputSha256 = "request-sha256"
            outputSha256 = "request-sha256"
        }
        displayableError = [pscustomobject]@{
            className = "pinball.common.error.DisplayableError"
            patchMethod = "getDisplayMessage"
            referenceSha256 = "display-sha256"
            inputSha256 = "display-sha256"
            outputSha256 = "diagnostic-display-sha256"
        }
    }
    $DecodeVerification = Assert-ClientPatchRemoteUtilDecodeDiagnosticEvidence -Evidence $DecodeEvidence
    Assert-TestCondition -Condition ($DecodeVerification.Verified -and $DecodeVerification.DiagnosticFields.Count -eq 9 -and $DecodeVerification.ChangedMethods[0] -ceq "requestCompleteHandler" -and $DecodeVerification.DisplayClassName -ceq "pinball.common.error.DisplayableError" -and $DecodeVerification.DisplayMethod -ceq "getDisplayMessage") -Message "响应和解码状态诊断证据没有验证字段和两个声明方法变化"
    $InvalidDecodeEvidence = $DecodeEvidence.PSObject.Copy()
    $InvalidDecodeEvidence.changesErrorTextOnly = $false
    $InvalidDecodeRejected = $false
    try {
        Assert-ClientPatchRemoteUtilDecodeDiagnosticEvidence -Evidence $InvalidDecodeEvidence | Out-Null
    } catch {
        $InvalidDecodeRejected = $true
    }
    Assert-TestCondition -Condition $InvalidDecodeRejected -Message "解码位置诊断证据接受了 ChangesErrorTextOnly=false"

    $AssetEvidence = [pscustomobject]@{
        className = "pinball.loading.initial.AssetExtractor"
        patchMethod = "start"
        digestVersion = 2
        methodOnly = $true
        requiresPreextractedBundle = $true
        conditionIndex = 42
        changedMethods = @("start")
        start = [pscustomobject]@{
            referenceSha256 = "asset-start-sha256"
            inputSha256 = "asset-start-sha256"
            outputSha256 = "asset-start-patched-sha256"
        }
    }
    $AssetVerification = Assert-ClientPatchAssetExtractorPreextractedEvidence -Evidence $AssetEvidence
    Assert-TestCondition -Condition ($AssetVerification.Verified -and $AssetVerification.RequiresPreextractedBundle -and $AssetVerification.ChangedMethods.Count -eq 1 -and $AssetVerification.ChangedMethods[0] -ceq "start") -Message "预展开资源证据没有限制 AssetExtractor.start 唯一变化"
    $InvalidAssetEvidence = $AssetEvidence.PSObject.Copy()
    $InvalidAssetEvidence.requiresPreextractedBundle = $false
    $InvalidAssetRejected = $false
    try {
        Assert-ClientPatchAssetExtractorPreextractedEvidence -Evidence $InvalidAssetEvidence | Out-Null
    } catch {
        $InvalidAssetRejected = $true
    }
    Assert-TestCondition -Condition $InvalidAssetRejected -Message "预展开资源证据接受了缺失运行前置条件的补丁"

    $VersionQueryEvidence = [pscustomobject]@{
        className = "pinball.gbits.logic.GbitsVersionLogic"
        patchMethod = "isQuerySuccess"
        digestVersion = 2
        methodOnly = $true
        removesSdkDummyEarlySuccess = $true
        preservesPublishTargetCheck = $true
        preservesVersionsCheck = $true
        changedMethods = @("isQuerySuccess")
        instructionRange = [pscustomobject]@{
            sdkDummyClassIndex = 2
            sdkDummyPropertyIndex = 3
            branchIndex = 5
        }
        isQuerySuccess = [pscustomobject]@{
            referenceSha256 = "version-query-before-sha256"
            inputSha256 = "version-query-before-sha256"
            outputSha256 = "version-query-after-sha256"
        }
    }
    $VersionQueryVerification = Assert-ClientPatchVersionQueryProbeEvidence -Evidence $VersionQueryEvidence
    Assert-TestCondition -Condition ($VersionQueryVerification.Verified -and $VersionQueryVerification.RemovesSdkDummyEarlySuccess -and $VersionQueryVerification.ChangedMethods.Count -eq 1 -and $VersionQueryVerification.ChangedMethods[0] -ceq "isQuerySuccess") -Message "版本查询证据没有限制 GbitsVersionLogic.isQuerySuccess 唯一变化"
    $InvalidVersionQueryEvidence = $VersionQueryEvidence.PSObject.Copy()
    $InvalidVersionQueryEvidence.removesSdkDummyEarlySuccess = $false
    $InvalidVersionQueryRejected = $false
    try {
        Assert-ClientPatchVersionQueryProbeEvidence -Evidence $InvalidVersionQueryEvidence | Out-Null
    } catch {
        $InvalidVersionQueryRejected = $true
    }
    Assert-TestCondition -Condition $InvalidVersionQueryRejected -Message "版本查询证据接受了未移除 sdkDummy 提前成功的补丁"

    $VersionUrlEvidence = [pscustomobject]@{
        className = "pinball.gbits.logic.GbitsVersionLogic"
        digestVersion = 2
        methodOnly = $true
        changedMethods = @("queryVersion", "onQueryErrorDefault")
        inputChangesFromReference = @()
        queryVersion = [pscustomobject]@{
            sourceUrl = "https://update.leiting.com/shijtswy/version/"
            targetUrl = "http://127.0.0.1:8001/shijtswy/version/"
            referenceSha256 = "version-url-query-reference"
            inputSha256 = "version-url-query-reference"
            outputSha256 = "version-url-query-output"
        }
        onQueryErrorDefault = [pscustomobject]@{
            sourceUrl = "https://update.roguelike.com/shijtswy/version/"
            targetUrl = "http://127.0.0.1:8001/shijtswy/version/"
            referenceSha256 = "version-url-backup-reference"
            inputSha256 = "version-url-backup-reference"
            outputSha256 = "version-url-backup-output"
        }
    }
    $VersionUrlVerification = Assert-ClientPatchVersionUrlAbcEvidence -Evidence $VersionUrlEvidence -TargetUrl "http://127.0.0.1:8001/shijtswy/version/"
    Assert-TestCondition -Condition ($VersionUrlVerification.Verified -and $VersionUrlVerification.ChangedMethods.Count -eq 2) -Message "版本地址证据没有限制两个目标方法"
    $InvalidVersionUrlEvidence = $VersionUrlEvidence.PSObject.Copy()
    $InvalidVersionUrlEvidence.changedMethods = @("queryVersion", "onQueryErrorDefault", "isQuerySuccess")
    $InvalidVersionUrlRejected = $false
    try {
        Assert-ClientPatchVersionUrlAbcEvidence -Evidence $InvalidVersionUrlEvidence -TargetUrl "http://127.0.0.1:8001/shijtswy/version/" | Out-Null
    } catch {
        $InvalidVersionUrlRejected = $true
    }
    Assert-TestCondition -Condition $InvalidVersionUrlRejected -Message "版本地址证据接受了未声明的方法变化"

    $InvalidRemoteUtilEvidence = $RemoteUtilEvidence.PSObject.Copy()
    $InvalidRemoteUtilEvidence.requestCompleteHandler = [pscustomobject]@{
        referenceSha256 = "response-before-sha256"
        inputSha256 = "response-before-sha256"
        outputSha256 = "response-after-sha256"
    }
    $InvalidEvidenceRejected = $false
    try {
        Assert-ClientPatchRemoteUtilAbcEvidence -Evidence $InvalidRemoteUtilEvidence | Out-Null
    } catch {
        $InvalidEvidenceRejected = $true
    }
    Assert-TestCondition -Condition $InvalidEvidenceRejected -Message "RemoteUtil 方法级补丁证据接受了被改写的响应完成方法"

    $TitleScenePath = Join-Path $TemporaryDirectory "title-scene.as"
    Set-TestTitleSceneFixture -ScriptPath $TitleScenePath
    $TitleSceneResult = Set-ClientPatchSdkDummyTitleSceneGuard -ScriptPath $TitleScenePath
    $TitleSceneAfter = [IO.File]::ReadAllText($TitleScenePath)
    $TitleSceneMethods = Get-ClientPatchSdkDummyTitleSceneMethods -Content $TitleSceneAfter
    $OpenMicroCommunity = $TitleSceneMethods.OpenMicroCommunity
    $OpenEntryGuard = [regex]::Match($OpenMicroCommunity, '(?s)public function openMicroCommunity\(param1:String\) : void\s*\{\s*if\(DevConfig\.sdkDummy\)\s*\{\s*return;\s*\}')
    $OpenJson = [regex]::Match($OpenMicroCommunity, 'JSON\.stringify\(')
    $OpenShowMicroCommunity = [regex]::Match($OpenMicroCommunity, 'LeitingSDKExtension\.getInstance\(\)\.showMicroCommunity\(_loc2_\);')
    Assert-TestCondition -Condition ($TitleSceneResult.OpenMicroCommunityMethodCount -eq 1 -and $TitleSceneResult.OpenMicroCommunityShowCallCount -eq 1 -and $TitleSceneResult.OpenMicroCommunityGuarded -and $OpenEntryGuard.Success -and $OpenJson.Success -and $OpenShowMicroCommunity.Success -and $OpenJson.Index -gt ($OpenEntryGuard.Index + $OpenEntryGuard.Length) -and $OpenShowMicroCommunity.Index -gt ($OpenEntryGuard.Index + $OpenEntryGuard.Length)) -Message "标题场景微社区守卫没有在 JSON 和原生调用前返回"

    $InitGbits = $TitleSceneMethods.InitGbits
    $InitNativeGuard = [regex]::Match($InitGbits, '(?s)if\(!DevConfig\.sdkDummy\)\s*\{\s*var _loc1_:LeitingSDKExtension = LeitingSDKExtension\.getInstance\(\);\s*_loc1_\.addEventListener\(CallBack\.MICROCOMMUNITYCALLBACK,onLeitingSDKStartCallBack\);\s*\}')
    $InitNativeExtension = [regex]::Match($InitGbits, 'LeitingSDKExtension\.getInstance\(\)')
    $InitListener = [regex]::Match($InitGbits, '_loc1_\.addEventListener\(CallBack\.MICROCOMMUNITYCALLBACK,onLeitingSDKStartCallBack\);')
    $ChannelInitIndex = $InitGbits.IndexOf('channelSDK.init("leiting",this,sdkInit);', [StringComparison]::Ordinal)
    $RemoteSignIndex = $InitGbits.IndexOf('queryRemoteSign();', [StringComparison]::Ordinal)
    $LoginOpenIndex = $InitGbits.IndexOf('openMicroCommunity("2");', [StringComparison]::Ordinal)
    Assert-TestCondition -Condition ($TitleSceneResult.InitGbitsMethodCount -eq 1 -and $TitleSceneResult.InitGbitsNativeExtensionCallCount -eq 1 -and $TitleSceneResult.InitGbitsListenerCount -eq 1 -and $TitleSceneResult.InitGbitsNativeExtensionGuarded -and $InitNativeGuard.Success -and $InitNativeExtension.Success -and $InitListener.Success -and $InitNativeExtension.Index -ge $InitNativeGuard.Index -and $InitListener.Index -ge $InitNativeGuard.Index -and $InitNativeExtension.Index -lt ($InitNativeGuard.Index + $InitNativeGuard.Length) -and $InitListener.Index -lt ($InitNativeGuard.Index + $InitNativeGuard.Length) -and $ChannelInitIndex -ge 0 -and $ChannelInitIndex -lt $InitNativeGuard.Index -and $RemoteSignIndex -ge 0 -and $RemoteSignIndex -lt $InitNativeGuard.Index -and $LoginOpenIndex -gt ($InitNativeGuard.Index + $InitNativeGuard.Length)) -Message "标题场景初始化守卫没有只包裹原生扩展和监听器"
    Assert-ClientPatchSdkDummyTitleSceneGuard -ScriptPath $TitleScenePath
    $TitleSceneLoginGateEvidence = Assert-ClientPatchTitleSceneLoginGateEvidence -ScriptPath $TitleScenePath
    Assert-TestCondition -Condition ($TitleSceneLoginGateEvidence.Verified -and $TitleSceneLoginGateEvidence.ButtonWiring.DisabledInitially -and $TitleSceneLoginGateEvidence.ButtonWiring.EnabledAfterSequence -and $TitleSceneLoginGateEvidence.LoginBranches.StartLoginServer.Index -gt $TitleSceneLoginGateEvidence.Guards.SdkLoggedIn.Index -and $TitleSceneLoginGateEvidence.LoginBranches.ManualLogin.Index -gt $TitleSceneLoginGateEvidence.Guards.SdkLoggedIn.Index) -Message "标题场景登录按钮门控证据没有覆盖按钮启用和两个登录分支"

    $ChannelMainPath = Join-Path $TemporaryDirectory "channel-main.as"
    Set-TestChannelMainFixture -ScriptPath $ChannelMainPath
    $ChannelMainResult = Set-ClientPatchSdkDummyRealRemoteBridge -ScriptPath $ChannelMainPath
    $ChannelMainAfter = [IO.File]::ReadAllText($ChannelMainPath)
    Assert-TestCondition -Condition ($ChannelMainResult.RealRemoteNullAssignmentCount -eq 1 -and $ChannelMainResult.ChannelSelectionCount -eq 1 -and $ChannelMainResult.SdkDummyUsesDummySdk) -Message "dummy 远端桥接没有替换渠道入口结构"
    Assert-TestCondition -Condition (-not $ChannelMainAfter.Contains("if(DevConfig.sdkDummy)`r`n         {`r`n            realRemote = null;", [StringComparison]::Ordinal) -and -not $ChannelMainAfter.Contains("if(DevConfig.sdkDummy)`n         {`n            realRemote = null;", [StringComparison]::Ordinal)) -Message "dummy 远端桥接仍清空真实远端对象"
    Assert-TestCondition -Condition ($ChannelMainAfter.Contains("if(DevConfig.sdkDummy || null == realRemote)", [StringComparison]::Ordinal) -and $ChannelMainAfter.Contains("sdk = new ChannelSDKDummy(realRemote,logic);", [StringComparison]::Ordinal)) -Message "dummy 远端桥接没有在 dummy 标志下保留真实远端并实例化渠道 dummy"

    $SdkDummyPath = Join-Path $TemporaryDirectory "channel-sdk-dummy.as"
    Set-TestChannelSdkDummyFixture -ScriptPath $SdkDummyPath
    $SdkDummyBefore = [IO.File]::ReadAllText($SdkDummyPath)
    $SdkDummyBeforeTestLogin = Get-ClientPatchSdkDummyTestLoginMethodContent -Content $SdkDummyBefore
    $SdkDummyLoginRequestProbeResult = Set-ClientPatchSdkDummyLoginRequestProbe -ScriptPath $SdkDummyPath
    $SdkDummyAfter = [IO.File]::ReadAllText($SdkDummyPath)
    $SdkDummyAfterTestLogin = Get-ClientPatchSdkDummyTestLoginMethodContent -Content $SdkDummyAfter
    Assert-TestCondition -Condition ($SdkDummyLoginRequestProbeResult.StartLoginMethodCount -eq 1 -and $SdkDummyLoginRequestProbeResult.TestLoginMethodPreserved -and $SdkDummyLoginRequestProbeResult.UsesClientTestLogin -and $SdkDummyLoginRequestProbeResult.LoginResponseMarkerCount -eq 1) -Message "登录请求探测没有记录内置登录请求保留或响应标记"
    Assert-TestCondition -Condition ($SdkDummyAfterTestLogin -ceq $SdkDummyBeforeTestLogin) -Message "登录请求探测修改了原始 testLogin 内容"
    Assert-TestCondition -Condition ($SdkDummyAfter.Contains("testConnect = true;", [StringComparison]::Ordinal) -and $SdkDummyAfter.Contains("testLogin();", [StringComparison]::Ordinal) -and $SdkDummyAfter.Contains("completeHandler = param1;", [StringComparison]::Ordinal) -and $SdkDummyAfter.Contains("lastOperationTime = getTimer() / 1000;", [StringComparison]::Ordinal)) -Message "登录请求探测没有设置请求完成回调"
    $SdkDummyStartLogin = [regex]::Match($SdkDummyAfter, '(?s)public function startLoginServer\(param1:Function\) : void.*?(?=\r?\n\s*public function|\r?\n\s*}\s*\r?\n})').Value
    Assert-TestCondition -Condition ($SdkDummyStartLogin.Contains("testLogin();", [StringComparison]::Ordinal) -and -not $SdkDummyStartLogin.Contains("nativeStartLoginServer", [StringComparison]::Ordinal) -and -not $SdkDummyStartLogin.Contains("remote.requestQueue.addRequest", [StringComparison]::Ordinal) -and -not $SdkDummyStartLogin.Contains("channels/channel_leiting/leiting_login", [StringComparison]::Ordinal)) -Message "登录请求探测没有只通过原有 testLogin 发起请求"
    Assert-TestCondition -Condition (-not $SdkDummyAfter.Contains("sdkLoginOk = true", [StringComparison]::Ordinal) -and -not $SdkDummyAfter.Contains("starpoint-local", [StringComparison]::Ordinal)) -Message "登录请求探测伪造了 SDK 登录成功状态或凭据"
    Assert-TestCondition -Condition ($SdkDummyAfter.Contains('titleScene.showInstantMessage("协议响应已到达",InstantMessagePosition.Center);', [StringComparison]::Ordinal)) -Message "登录请求探测缺少响应到达标记"
    Assert-ClientPatchSdkDummyLoginRequestProbe -ScriptPath $SdkDummyPath

    $DefaultMode = Get-ClientPatchSdkDummyLoginRequestProbeMode -ProbeSdkDummyLoginRequest $false
    Assert-TestCondition -Condition ($DefaultMode.Name -ceq "none" -and -not $DefaultMode.PatchSdkDummy -and -not $DefaultMode.PatchChannelMainForSdkDummy -and -not $DefaultMode.PatchDummyLoginRequestProbe -and -not $DefaultMode.PatchSdkDummyNativeExtensionGuard -and -not $DefaultMode.PatchRemoteUtilRequestGuard -and -not $DefaultMode.PatchSdkDummyTitleSceneGuard) -Message "默认模式不应替换任何 SDK 登录路径"
    $ClassSelectionArguments = @{
        VersionClassName = "version"
        ApiConfigClassName = "api"
        DevConfigClassName = "dev-config"
        TitleSceneClassName = "title-scene"
        ChannelMainClassName = "channel-main"
        ChannelDummyClassName = "channel-dummy"
    }
    $DefaultSelectedClasses = @(Get-ClientPatchSelectedClassNames -PatchMode $DefaultMode @ClassSelectionArguments)
    Assert-TestCondition -Condition ($DefaultSelectedClasses.Count -eq 2 -and $DefaultSelectedClasses[0] -ceq "version" -and $DefaultSelectedClasses[1] -ceq "api") -Message "默认模式必须只选择版本和 API 类"
    foreach ($SdkClassName in @("dev-config", "remote-util", "title-scene", "channel-main", "channel-dummy")) {
        Assert-TestCondition -Condition ($DefaultSelectedClasses -notcontains $SdkClassName) -Message "默认模式错误选择 SDK 类: $SdkClassName"
    }
    $ProbeSelectedClasses = @(Get-ClientPatchSelectedClassNames -PatchMode $ProbeMode @ClassSelectionArguments)
    $ExpectedProbeClasses = @("version", "api", "dev-config", "title-scene", "channel-main", "channel-dummy")
    Assert-TestCondition -Condition ($ProbeSelectedClasses.Count -eq $ExpectedProbeClasses.Count -and ($ProbeSelectedClasses -join "|") -ceq ($ExpectedProbeClasses -join "|")) -Message "探测模式没有按固定顺序选择全部 SDK 类"
    Assert-TestCondition -Condition (@($ProbeSelectedClasses | Group-Object | Where-Object Count -gt 1).Count -eq 0) -Message "探测模式重复选择 SDK 类"
    Assert-TestCondition -Condition ($ProbeSelectedClasses -notcontains "remote-util") -Message "探测模式仍让 FFDec 重新导入 RemoteUtil"
    $ProductionSelectedClasses = @(Get-ClientPatchSelectedClassNames -PatchMode $DefaultMode @ClassSelectionArguments -SkipVersionClass)
    Assert-TestCondition -Condition ($ProductionSelectedClasses.Count -eq 1 -and $ProductionSelectedClasses[0] -ceq "api") -Message "正式模式必须跳过 GbitsVersionLogic 的 FFDec 导出"

    $DefaultDecodeMode = Get-ClientPatchMessagePackDecodeDiagnosticMode -ProbeMessagePackDecodePosition $false -ProbeSdkDummyLoginRequest $false
    Assert-TestCondition -Condition ($DefaultDecodeMode.Name -ceq "none" -and -not $DefaultDecodeMode.Enabled -and -not $DefaultDecodeMode.PatchRemoteUtilResponseError -and -not $DefaultDecodeMode.PatchDisplayableErrorMessage) -Message "默认模式不应修改响应解码错误路径"
    $DecodeMode = Get-ClientPatchMessagePackDecodeDiagnosticMode -ProbeMessagePackDecodePosition $true -ProbeSdkDummyLoginRequest $true
    Assert-TestCondition -Condition ($DecodeMode.Name -ceq "messagepack-decode-position" -and $DecodeMode.Enabled -and $DecodeMode.PatchRemoteUtilResponseError -and $DecodeMode.PatchDisplayableErrorMessage) -Message "MessagePack 解码位置诊断模式未启用"
    $DecodeModeError = $null
    try {
        Get-ClientPatchMessagePackDecodeDiagnosticMode -ProbeMessagePackDecodePosition $true -ProbeSdkDummyLoginRequest $false | Out-Null
    } catch {
        $DecodeModeError = $_.Exception.Message
    }
    Assert-TestCondition -Condition ($DecodeModeError -eq "MessagePack 解码位置诊断需要同时启用 ProbeSdkDummyLoginRequest") -Message "MessagePack 解码位置诊断未拒绝缺少登录探测的调用"

    $DefaultPreextractedMode = Get-ClientPatchPreextractedBundleMode -UsePreextractedBundle $false
    Assert-TestCondition -Condition ($DefaultPreextractedMode.Name -ceq "none" -and -not $DefaultPreextractedMode.Enabled -and -not $DefaultPreextractedMode.PatchAssetExtractorStart -and -not $DefaultPreextractedMode.RequiresPreextractedBundle) -Message "默认模式不应跳过内置资源解包"
    $PreextractedMode = Get-ClientPatchPreextractedBundleMode -UsePreextractedBundle $true
    Assert-TestCondition -Condition ($PreextractedMode.Name -ceq "preextracted-bundle" -and $PreextractedMode.Enabled -and $PreextractedMode.PatchAssetExtractorStart -and $PreextractedMode.RequiresPreextractedBundle) -Message "预展开内置资源模式未声明方法补丁和运行前置条件"

    $DefaultVersionQueryMode = Get-ClientPatchVersionQueryProbeMode -ProbeVersionQuery $false -ProbeSdkDummyLoginRequest $false
    Assert-TestCondition -Condition ($DefaultVersionQueryMode.Name -ceq "none" -and -not $DefaultVersionQueryMode.Enabled -and -not $DefaultVersionQueryMode.PatchVersionQuerySuccess) -Message "默认模式不应修改版本查询成功判断"
    $VersionQueryMode = Get-ClientPatchVersionQueryProbeMode -ProbeVersionQuery $true -ProbeSdkDummyLoginRequest $true
    Assert-TestCondition -Condition ($VersionQueryMode.Name -ceq "sdk-dummy-version-query" -and $VersionQueryMode.Enabled -and $VersionQueryMode.PatchVersionQuerySuccess) -Message "版本查询诊断模式未启用"
    $VersionQueryModeError = $null
    try {
        Get-ClientPatchVersionQueryProbeMode -ProbeVersionQuery $true -ProbeSdkDummyLoginRequest $false | Out-Null
    } catch {
        $VersionQueryModeError = $_.Exception.Message
    }
    Assert-TestCondition -Condition ($VersionQueryModeError -eq "版本查询诊断需要同时启用 ProbeSdkDummyLoginRequest") -Message "版本查询诊断未拒绝缺少登录探测的调用"

    $PatchContent = [IO.File]::ReadAllText($PatchScriptPath)
    Assert-TestCondition -Condition ($PatchContent.Contains("SdkDummyLoginRequestProbeEnabled", [StringComparison]::Ordinal) -and $PatchContent.Contains("SdkDummyTitleSceneGuardEnabled", [StringComparison]::Ordinal) -and $PatchContent.Contains("RemoteUtilRequestGuardEnabled", [StringComparison]::Ordinal) -and $PatchContent.Contains("PreextractedBundleEnabled", [StringComparison]::Ordinal) -and $PatchContent.Contains("VersionUrlAbc", [StringComparison]::Ordinal) -and $PatchContent.Contains("Enabled = [bool]`$VersionUrlPatchEnabled", [StringComparison]::Ordinal) -and $PatchContent.Contains('Strategy = "abc-method-body"', [StringComparison]::Ordinal) -and $PatchContent.Contains("DynamicVerificationPerformed = `$false", [StringComparison]::Ordinal) -and $PatchContent.Contains("VerifiesServerResponseOrSdkLoginState = `$false", [StringComparison]::Ordinal)) -Message "补丁 manifest 未声明登录请求探测和版本地址补丁证据"
    $RequiredPatchFragments = @(
        '$ChannelMainClassName',
        '$RemoteUtilClassName',
        '$TitleSceneClassName',
        'RemoteUtilAbcPatch.java',
        'RemoteUtilDecodeDiagnosticPatch.java',
        'AssetExtractorPreextractedBundlePatch.java',
        'GbitsVersionQueryProbePatch.java',
        'GbitsVersionUrlAbcPatch.java',
        'AbcMethodDigest.java',
        'RemoteUtilMethodDigest.java',
        'MethodDigestSource',
        'Assert-ClientPatchRemoteUtilAbcEvidence',
        'Assert-ClientPatchRemoteUtilDecodeDiagnosticEvidence',
        'Assert-ClientPatchAssetExtractorPreextractedEvidence',
        'Assert-ClientPatchVersionQueryProbeEvidence',
        'Assert-ClientPatchVersionUrlAbcEvidence',
        'Set-ClientPatchSdkDummyRealRemoteBridge',
        'Set-ClientPatchSdkDummyLoginRequestProbe',
        'Set-ClientPatchSdkDummyNativeExtensionGuard',
        'Set-ClientPatchSdkDummyTitleSceneGuard',
        'Assert-ClientPatchSdkDummyLoginRequestProbe',
        'Assert-ClientPatchSdkDummyTitleSceneGuard',
        'ChangesErrorTextOnly = [bool]$MessagePackDecodeDiagnosticMode.PatchRemoteUtilResponseError',
        'DisplaysInternalErrorMessage = [bool]$MessagePackDecodeDiagnosticMode.PatchDisplayableErrorMessage',
        'RequiresPreextractedBundle = [bool]$PreextractedBundleMode.RequiresPreextractedBundle',
        'RequiresSdkDummyLoginProbe = [bool]$VersionQueryProbeMode.PatchVersionQuerySuccess',
        'Assert-ClientPatchRemoteUtilDecodeDiagnosticEvidence -Evidence $Evidence'
    )
    foreach ($RequiredPatchFragment in $RequiredPatchFragments) {
        Assert-TestCondition -Condition ($PatchContent.Contains($RequiredPatchFragment, [StringComparison]::Ordinal)) -Message "补丁缺少方法级守卫或摘要证据: fragment=$RequiredPatchFragment"
    }
    Assert-TestCondition -Condition (-not $PatchContent.Contains('Set-ClientPatchSdkDummyRequestTransportGuard', [StringComparison]::Ordinal) -and -not $PatchContent.Contains('Set-ClientPatchSdkDummyResponseStageProbe', [StringComparison]::Ordinal) -and -not $PatchContent.Contains('ResponseStageProbe', [StringComparison]::Ordinal) -and -not $PatchContent.Contains('[protocol-lab] response-callback', [StringComparison]::Ordinal)) -Message "补丁仍通过不完整 ActionScript 源码改写 RemoteUtil"
    Assert-TestCondition -Condition (-not $PatchContent.Contains("BypassSdkLogin", [StringComparison]::Ordinal) -and -not $PatchContent.Contains("BridgeSdkLogin", [StringComparison]::Ordinal) -and -not $PatchContent.Contains("sdkLoginOk = true", [StringComparison]::Ordinal) -and -not $PatchContent.Contains("starpoint-local", [StringComparison]::Ordinal)) -Message "补丁仍包含 SDK 登录成功伪造路径"
    Assert-TestCondition -Condition (-not $PatchContent.Contains('-FilePath $FfdecPath', [StringComparison]::Ordinal) -and -not $PatchContent.Contains('-FilePath $ApkSignerPath', [StringComparison]::Ordinal)) -Message "补丁仍通过批处理启动 FFDec 或 apksigner"
    Assert-TestCondition -Condition ($PatchContent.Contains("-jar", [StringComparison]::Ordinal) -and $PatchContent.Contains('$FfdecJarPath', [StringComparison]::Ordinal) -and $PatchContent.Contains('$ApkSignerJarPath', [StringComparison]::Ordinal)) -Message "补丁没有通过 Java jar 调用 FFDec 或 apksigner"

    $SkippedRemoteUtilJavaTest = Invoke-TestClientAbcPatches `
        -PatchSourcePath (Join-Path $PSScriptRoot "RemoteUtilAbcPatch.java") `
        -AbcDigestSourcePath (Join-Path $PSScriptRoot "AbcMethodDigest.java") `
        -DigestSourcePath (Join-Path $PSScriptRoot "RemoteUtilMethodDigest.java") `
        -TestSourcePath (Join-Path $PSScriptRoot "RemoteUtilAbcPatchTest.java") `
        -DecodePatchSourcePath (Join-Path $PSScriptRoot "RemoteUtilDecodeDiagnosticPatch.java") `
        -DecodeTestSourcePath (Join-Path $PSScriptRoot "RemoteUtilDecodeDiagnosticPatchTest.java") `
        -AssetPatchSourcePath (Join-Path $PSScriptRoot "AssetExtractorPreextractedBundlePatch.java") `
        -AssetTestSourcePath (Join-Path $PSScriptRoot "AssetExtractorPreextractedBundlePatchTest.java") `
        -VersionPatchSourcePath (Join-Path $PSScriptRoot "GbitsVersionQueryProbePatch.java") `
        -VersionTestSourcePath (Join-Path $PSScriptRoot "GbitsVersionQueryProbePatchTest.java") `
        -VersionUrlPatchSourcePath (Join-Path $PSScriptRoot "GbitsVersionUrlAbcPatch.java") `
        -VersionUrlTestSourcePath (Join-Path $PSScriptRoot "GbitsVersionUrlAbcPatchTest.java") `
        -TemporaryDirectory $TemporaryDirectory
    Assert-TestCondition -Condition (-not $SkippedRemoteUtilJavaTest.Performed) -Message "外部 SWF 未提供时不应运行 Java 回归测试"

    $RemoteUtilJavaTest = Invoke-TestClientAbcPatches `
        -PatchSourcePath (Join-Path $PSScriptRoot "RemoteUtilAbcPatch.java") `
        -AbcDigestSourcePath (Join-Path $PSScriptRoot "AbcMethodDigest.java") `
        -DigestSourcePath (Join-Path $PSScriptRoot "RemoteUtilMethodDigest.java") `
        -TestSourcePath (Join-Path $PSScriptRoot "RemoteUtilAbcPatchTest.java") `
        -DecodePatchSourcePath (Join-Path $PSScriptRoot "RemoteUtilDecodeDiagnosticPatch.java") `
        -DecodeTestSourcePath (Join-Path $PSScriptRoot "RemoteUtilDecodeDiagnosticPatchTest.java") `
        -AssetPatchSourcePath (Join-Path $PSScriptRoot "AssetExtractorPreextractedBundlePatch.java") `
        -AssetTestSourcePath (Join-Path $PSScriptRoot "AssetExtractorPreextractedBundlePatchTest.java") `
        -VersionPatchSourcePath (Join-Path $PSScriptRoot "GbitsVersionQueryProbePatch.java") `
        -VersionTestSourcePath (Join-Path $PSScriptRoot "GbitsVersionQueryProbePatchTest.java") `
        -VersionUrlPatchSourcePath (Join-Path $PSScriptRoot "GbitsVersionUrlAbcPatch.java") `
        -VersionUrlTestSourcePath (Join-Path $PSScriptRoot "GbitsVersionUrlAbcPatchTest.java") `
        -LibraryJarPath $FfdecLibraryJarPath `
        -SwfPath $ReferenceSwfPath `
        -TemporaryDirectory $TemporaryDirectory
    if (-not $RemoteUtilJavaTest.Performed) {
        "RemoteUtilAbcPatchTest: SKIP external SWF not supplied"
    } else {
        $RemoteUtilJavaTest.Output
    }

    "test-cn-sdk-login-bridge: PASS"
} finally {
    if (Test-Path -LiteralPath $TemporaryDirectory) {
        $RelativeDeleteTarget = [IO.Path]::GetRelativePath($TemporaryRoot, $TemporaryDirectory)
        if ([IO.Path]::IsPathRooted($RelativeDeleteTarget) -or $RelativeDeleteTarget -eq "." -or $RelativeDeleteTarget -eq ".." -or $RelativeDeleteTarget.StartsWith("..$([IO.Path]::DirectorySeparatorChar)", [StringComparison]::Ordinal)) {
            throw "测试临时目录不在系统临时目录内: $TemporaryDirectory"
        }
        Remove-Item -LiteralPath $TemporaryDirectory -Recurse -Force
    }
}
# //// /验证 SDK 登录请求探测模式 ////
