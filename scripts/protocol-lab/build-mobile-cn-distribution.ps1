# audience: external
# # build-mobile-cn-distribution
# 双端使用干净提交和相同的个人服务及资源基线, 按顺序生成 Android 合并包与 unsigned IPA.
# 工作目录与版本分发目录位于同一卷. 分发目录通过目录移动发布, 其中包含一个 APK 和一个 IPA.

[CmdletBinding()]
param(
    [Parameter(Mandatory)][string]$AndroidRepositoryRoot,
    [Parameter(Mandatory)][string]$IosRepositoryRoot,
    [Parameter(Mandatory)][string]$InputApkPath,
    [Parameter(Mandatory)][string]$InputIpa,
    [Parameter(Mandatory)][string]$AndroidCdnRoot,
    [Parameter(Mandatory)][string]$IosCdnRoot,
    [Parameter(Mandatory)][ValidatePattern('^[A-Za-z0-9-]+(?:\.[A-Za-z0-9-]+)+$')][string]$BundleId,
    [Parameter(Mandatory)][string]$DisplayName,
    [Parameter(Mandatory)][ValidatePattern('^\d+\.\d+\.\d+$')][string]$ReleaseVersion,
    [Parameter(Mandatory)][ValidateRange(1, [int]::MaxValue)][int]$VersionCode,
    [Parameter(Mandatory)][string]$OutputRoot,
    [Parameter(Mandatory)][string]$WorkDirectory,
    [ValidatePattern('^[A-Za-z0-9][A-Za-z0-9._:-]*$')][string]$SshHost,
    [string]$AndroidSdkRoot,
    [string]$StarviewToolsRoot,
    [string]$JavaHome,
    [string]$CargoTargetDirectory,
    [string]$KeystorePath,
    [string]$KeystorePasswordFile,
    [string]$CompanionKeystorePath,
    [string]$CompanionKeystorePassword
)

$ErrorActionPreference = 'Stop'
Set-StrictMode -Version Latest

# //// 读取干净工作树的当前提交 [@x380kkm 2026-09-28] ////
function Get-MobileBuildCommit {
    param([string]$RepositoryRoot)
    $commit = & git -C $RepositoryRoot rev-parse HEAD
    $exitCode = $LASTEXITCODE
    if ($exitCode -ne 0) { throw "读取构建提交失败: $RepositoryRoot" }
    $status = @(& git -C $RepositoryRoot status --porcelain --untracked-files=all)
    $exitCode = $LASTEXITCODE
    if ($exitCode -ne 0) { throw "读取工作树状态失败: $RepositoryRoot" }
    if ($status.Count -gt 0) { throw "构建工作树包含未提交改动: $RepositoryRoot" }
    ([string]$commit).Trim()
}

# //// 校验移动路径位于指定目录内 [@x380kkm 2026-09-28] ////
function Assert-MobileChildPath {
    param([string]$Root, [string]$Path)
    $relative = [IO.Path]::GetRelativePath([IO.Path]::GetFullPath($Root), [IO.Path]::GetFullPath($Path))
    if ([IO.Path]::IsPathRooted($relative) -or $relative -in @('.', '..') -or
        $relative.StartsWith("..$([IO.Path]::DirectorySeparatorChar)", [StringComparison]::Ordinal)) {
        throw "路径超出指定目录: $Path, 目录: $Root"
    }
}

# //// 执行现有构建入口并保存进程输出 [@x380kkm 2026-09-28] ////
function Invoke-MobileBuild {
    param([string]$ScriptPath, [hashtable]$Parameters, [string]$RepositoryRoot, [string]$LogPrefix)
    $arguments = [Collections.Generic.List[string]]::new()
    $arguments.Add('-File')
    $arguments.Add($ScriptPath)
    foreach ($parameter in $Parameters.GetEnumerator()) {
        $arguments.Add("-$($parameter.Key)")
        $arguments.Add([string]$parameter.Value)
    }
    $result = Invoke-IosValidationProcess -FilePath 'pwsh' -Arguments $arguments.ToArray() `
        -WorkingDirectory $RepositoryRoot -TimeoutSeconds 7200
    $result.Stdout | Set-Content -LiteralPath "$LogPrefix.stdout.log" -Encoding UTF8
    $result.Stderr | Set-Content -LiteralPath "$LogPrefix.stderr.log" -Encoding UTF8
    $result
}

# //// 读取 Android 构建输出末尾的分发报告 [@x380kkm 2026-09-28] ////
function Read-AndroidMobileReport {
    param([string]$Output)
    $objects = [regex]::Matches($Output, '(?m)^\{')
    if ($objects.Count -eq 0) { throw 'Android 构建输出缺少分发报告.' }
    $Output.Substring($objects[$objects.Count - 1].Index) | ConvertFrom-Json
}

# //// 核对 iOS 进程与候选阶段的完成状态 [@x380kkm 2026-09-28] ////
function Read-IosMobileReport {
    param([string]$ReportPath, [int]$ExitCode)
    if (-not (Test-Path -LiteralPath $ReportPath -PathType Leaf)) {
        throw "iOS 构建退出码为 $ExitCode, 候选报告缺失: $ReportPath"
    }
    $candidate = Get-Content -LiteralPath $ReportPath -Encoding UTF8 -Raw | ConvertFrom-Json
    if ($ExitCode -ne 0 -or $candidate.status -ne 'passed') {
        $package = @($candidate.stages | Where-Object { $_.stage -eq 'PACKAGE' -and $_.status -eq 'passed' })
        $artifact = if ($package.Count -gt 0) { "IPA 已生成并保留在 $($candidate.output_ipa)." } else { 'IPA 构建尚未完成.' }
        throw "iOS 构建失败: exit=$ExitCode, stage=$($candidate.first_failure), code=$($candidate.root_error_code). $artifact 报告: $ReportPath"
    }
    $candidate
}

# //// 同卷移动两份安装包并发布完整版本目录 [@x380kkm 2026-09-28] ////
function Publish-MobileArtifacts {
    param([string]$AndroidApk, [string]$IosIpa, [string]$WorkDirectory, [string]$OutputDirectory, [string]$ReleaseVersion)
    $work = [IO.Path]::GetFullPath($WorkDirectory)
    $destination = [IO.Path]::GetFullPath($OutputDirectory)
    if ([IO.Path]::GetPathRoot($work) -ine [IO.Path]::GetPathRoot($destination)) {
        throw '工作目录与分发目录必须位于同一卷.'
    }
    if (Test-Path -LiteralPath $destination) { throw "版本分发目录已存在: $destination" }
    $staging = Join-Path $work 'publish'
    if (Test-Path -LiteralPath $staging) { throw "发布暂存目录已存在: $staging" }
    Assert-MobileChildPath -Root $work -Path $staging
    $files = @(
        @{ Source = [IO.Path]::GetFullPath($AndroidApk); Name = "sp-$ReleaseVersion.apk" },
        @{ Source = [IO.Path]::GetFullPath($IosIpa); Name = "sp-$ReleaseVersion.ipa" }
    )
    foreach ($file in $files) {
        Assert-MobileChildPath -Root $work -Path $file.Source
        Assert-MobileChildPath -Root $staging -Path (Join-Path $staging $file.Name)
        if (-not (Test-Path -LiteralPath $file.Source -PathType Leaf) -or
            (Get-Item -LiteralPath $file.Source).Length -le 0) {
            throw "安装包缺失或为空: $($file.Source)"
        }
    }
    New-Item -ItemType Directory -Path $staging | Out-Null
    New-Item -ItemType Directory -Path (Split-Path -Parent $destination) -Force | Out-Null
    foreach ($file in $files) {
        [IO.File]::Move($file.Source, (Join-Path $staging $file.Name))
    }
    $entries = @(Get-ChildItem -LiteralPath $staging -Force)
    if ($entries.Count -ne 2 -or @($entries | Where-Object { $_.PSIsContainer -or $_.Name -cnotin $files.Name }).Count -gt 0) {
        throw "发布暂存目录内容异常: $staging"
    }
    if (Test-Path -LiteralPath $destination) { throw "版本分发目录已存在: $destination" }
    [IO.Directory]::Move($staging, $destination)
    [ordered]@{
        apk = Join-Path $destination "sp-$ReleaseVersion.apk"
        ipa = Join-Path $destination "sp-$ReleaseVersion.ipa"
    }
}

$AndroidRepositoryRoot = (Resolve-Path -LiteralPath $AndroidRepositoryRoot).ProviderPath
$IosRepositoryRoot = (Resolve-Path -LiteralPath $IosRepositoryRoot).ProviderPath
$InputApkPath = (Resolve-Path -LiteralPath $InputApkPath).ProviderPath
$InputIpa = (Resolve-Path -LiteralPath $InputIpa).ProviderPath
$AndroidCdnRoot = (Resolve-Path -LiteralPath $AndroidCdnRoot).ProviderPath
$IosCdnRoot = (Resolve-Path -LiteralPath $IosCdnRoot).ProviderPath
foreach ($inputFile in @($InputApkPath, $InputIpa)) {
    if (-not (Test-Path -LiteralPath $inputFile -PathType Leaf)) { throw "安装包输入必须为文件: $inputFile" }
}
foreach ($inputDirectory in @($AndroidRepositoryRoot, $IosRepositoryRoot, $AndroidCdnRoot, $IosCdnRoot)) {
    if (-not (Test-Path -LiteralPath $inputDirectory -PathType Container)) { throw "构建输入必须为目录: $inputDirectory" }
}
$OutputRoot = [IO.Path]::GetFullPath($OutputRoot)
$WorkDirectory = [IO.Path]::GetFullPath($WorkDirectory)
$outputDirectory = Join-Path $OutputRoot $ReleaseVersion
Assert-MobileChildPath -Root $OutputRoot -Path $outputDirectory
if ((Test-Path -LiteralPath $OutputRoot) -and -not (Test-Path -LiteralPath $OutputRoot -PathType Container)) {
    throw "分发根路径必须为目录: $OutputRoot"
}
if (Test-Path -LiteralPath $outputDirectory) { throw "版本分发目录已存在: $outputDirectory" }
if (Test-Path -LiteralPath $WorkDirectory) { throw "构建工作目录已存在, 请先读取其中的构建回执: $WorkDirectory" }
if ([IO.Path]::GetPathRoot($WorkDirectory) -ine [IO.Path]::GetPathRoot($OutputRoot)) {
    throw '工作目录与分发目录必须位于同一卷.'
}
foreach ($root in @($AndroidRepositoryRoot, $IosRepositoryRoot, $outputDirectory)) {
    $relative = [IO.Path]::GetRelativePath($root, $WorkDirectory)
    if ($relative -eq '.' -or (-not [IO.Path]::IsPathRooted($relative) -and $relative -ne '..' -and
        -not $relative.StartsWith("..$([IO.Path]::DirectorySeparatorChar)", [StringComparison]::Ordinal))) {
        throw "工作目录必须位于仓库和分发目录之外: $WorkDirectory"
    }
}
$androidCommit = Get-MobileBuildCommit -RepositoryRoot $AndroidRepositoryRoot
$iosCommit = Get-MobileBuildCommit -RepositoryRoot $IosRepositoryRoot
Import-Module (Join-Path $AndroidRepositoryRoot 'scripts/protocol-lab/android-packaging-paths.psm1') -Force
Import-Module (Join-Path $IosRepositoryRoot 'scripts/protocol-lab/ios-validation.psm1') -Force

$temporaryDirectory = Join-Path $WorkDirectory 'temp'
New-Item -ItemType Directory -Path $WorkDirectory | Out-Null
New-Item -ItemType Directory -Path $temporaryDirectory | Out-Null
$previousTemp = [Environment]::GetEnvironmentVariable('TEMP', 'Process')
$previousTmp = [Environment]::GetEnvironmentVariable('TMP', 'Process')
$reportPath = Join-Path $WorkDirectory 'mobile-distribution.json'
$report = [ordered]@{
    status = 'failed'
    release_version = $ReleaseVersion
    version_code = $VersionCode
    android_commit = $androidCommit
    ios_commit = $iosCommit
    work_directory = $WorkDirectory
    output_directory = $outputDirectory
    android_exit_code = $null
    ios_exit_code = $null
    ios_candidate_report = Join-Path $WorkDirectory 'ios/candidate-build.json'
    artifacts = $null
    error = $null
}

# //// 在进程临时目录中顺序构建双端安装包 [@x380kkm 2026-09-28] ////
try {
    [Environment]::SetEnvironmentVariable('TEMP', $temporaryDirectory, 'Process')
    [Environment]::SetEnvironmentVariable('TMP', $temporaryDirectory, 'Process')
    $baseline = Assert-AndroidPersonalServiceBaseline -RepositoryRoot $AndroidRepositoryRoot -BaselineRef $iosCommit
    $baseline | ConvertTo-Json -Depth 8 | Set-Content -LiteralPath (Join-Path $WorkDirectory 'shared-baseline.json') -Encoding UTF8
    $androidOutput = Join-Path $WorkDirectory 'android'
    $androidParameters = @{
        InputApkPath = $InputApkPath; SourceCdnRoot = $AndroidCdnRoot; OutputDirectory = $androidOutput
        ServiceBaselineRef = $iosCommit; ReleaseVersion = $ReleaseVersion; VersionCode = $VersionCode
    }
    foreach ($name in @('AndroidSdkRoot', 'StarviewToolsRoot', 'JavaHome', 'CargoTargetDirectory',
        'KeystorePath', 'KeystorePasswordFile', 'CompanionKeystorePath', 'CompanionKeystorePassword')) {
        if ($PSBoundParameters.ContainsKey($name)) { $androidParameters[$name] = $PSBoundParameters[$name] }
    }
    Write-Host "构建 Android $ReleaseVersion. 工作目录: $WorkDirectory"
    $android = Invoke-MobileBuild -ScriptPath (Join-Path $AndroidRepositoryRoot 'scripts/protocol-lab/build-android-cn-distribution.ps1') `
        -Parameters $androidParameters -RepositoryRoot $AndroidRepositoryRoot -LogPrefix (Join-Path $WorkDirectory 'android')
    $report.android_exit_code = $android.ExitCode
    if ($android.ExitCode -ne 0) { throw "Android 构建失败: exit=$($android.ExitCode). 日志: $WorkDirectory/android.stderr.log" }
    $androidReport = Read-AndroidMobileReport -Output $android.Stdout
    $androidReport | ConvertTo-Json -Depth 12 | Set-Content -LiteralPath (Join-Path $WorkDirectory 'android-distribution.json') -Encoding UTF8
    if ($androidReport.apk.version_name -cne $ReleaseVersion -or $androidReport.apk.version_code -ne $VersionCode -or
        $androidReport.mode -cne 'single-apk-tail-cdn' -or
        $androidReport.source.personal_service.source_commit -cne $androidCommit -or
        $androidReport.source.personal_service.service_baseline_commit -cne $iosCommit) {
        throw 'Android 成包报告与指定版本及提交不一致.'
    }
    $androidApk = Join-Path $androidOutput "sp-$ReleaseVersion.apk"

    $iosOutput = Join-Path $WorkDirectory 'ios'
    $iosParameters = @{
        InputIpa = $InputIpa; CnCdnBundle = $IosCdnRoot; BundleId = $BundleId; DisplayName = $DisplayName
        ReleaseVersion = $ReleaseVersion; Commit = $iosCommit; OutputDirectory = $iosOutput
    }
    if ($PSBoundParameters.ContainsKey('SshHost')) { $iosParameters.SshHost = $SshHost }
    Write-Host "构建 iOS $ReleaseVersion. 工作目录: $WorkDirectory"
    $ios = Invoke-MobileBuild -ScriptPath (Join-Path $IosRepositoryRoot 'scripts/protocol-lab/build-ios-cn-candidate.ps1') `
        -Parameters $iosParameters -RepositoryRoot $IosRepositoryRoot -LogPrefix (Join-Path $WorkDirectory 'ios')
    $report.ios_exit_code = $ios.ExitCode
    $iosReport = Read-IosMobileReport -ReportPath $report.ios_candidate_report -ExitCode $ios.ExitCode
    if ($iosReport.commit -cne $iosCommit -or $iosReport.release_version -cne $ReleaseVersion) {
        throw 'iOS 成包报告与指定版本及提交不一致.'
    }
    $iosIpa = [IO.Path]::GetFullPath([string]$iosReport.output_ipa)
    Assert-MobileChildPath -Root $iosOutput -Path $iosIpa
    if ((Get-MobileBuildCommit -RepositoryRoot $AndroidRepositoryRoot) -cne $androidCommit -or
        (Get-MobileBuildCommit -RepositoryRoot $IosRepositoryRoot) -cne $iosCommit) {
        throw '构建期间仓库提交发生变化, 安装包保留在工作目录.'
    }
    $report.artifacts = Publish-MobileArtifacts -AndroidApk $androidApk -IosIpa $iosIpa `
        -WorkDirectory $WorkDirectory -OutputDirectory $outputDirectory -ReleaseVersion $ReleaseVersion
    $report.status = 'passed'
} catch {
    $report.error = Protect-IosValidationText -Text $_.Exception.Message
} finally {
    [Environment]::SetEnvironmentVariable('TEMP', $previousTemp, 'Process')
    [Environment]::SetEnvironmentVariable('TMP', $previousTmp, 'Process')
    $report | ConvertTo-Json -Depth 8 | Set-Content -LiteralPath $reportPath -Encoding UTF8
}
# //// /在进程临时目录中顺序构建双端安装包 ////

$report | ConvertTo-Json -Depth 8
if ($report.status -ne 'passed') { throw "双端出包失败. 回执: $reportPath. $($report.error)" }
