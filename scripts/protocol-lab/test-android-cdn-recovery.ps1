# audience: internal
# # test-android-cdn-recovery
# 该脚本使用本机 Android SDK 编译类型并回放 CDN 文件系统中断恢复.

[CmdletBinding()]
param([string]$AndroidSdkRoot)

$ErrorActionPreference = 'Stop'
$RepositoryRoot = [IO.Path]::GetFullPath((Join-Path $PSScriptRoot '../..'))
Import-Module (Join-Path $PSScriptRoot 'android-packaging-paths.psm1')
if ([string]::IsNullOrWhiteSpace($AndroidSdkRoot)) {
    $WorkspaceRoot = Resolve-AndroidPackagingWorkspaceRoot -RepositoryRoot $RepositoryRoot
    $AndroidSdkRoot = Join-Path $WorkspaceRoot 'artifacts/android-sdk/sdk'
}
$AndroidJar = Join-Path $AndroidSdkRoot 'platforms/android-35/android.jar'
if (-not (Test-Path -LiteralPath $AndroidJar -PathType Leaf)) {
    throw "Android SDK 编译包不存在: $AndroidJar"
}

# //// 编译并回放临时目录上的恢复行为 [@x380kkm 2026-09-10] ////
$CompilationDirectory = Join-Path ([IO.Path]::GetTempPath()) ('starpoint-cdn-test-' + [Guid]::NewGuid().ToString('N'))
New-Item -ItemType Directory -Path $CompilationDirectory | Out-Null
$Sources = @(
    (Join-Path $RepositoryRoot 'platforms/android/CompanionHost/src/dev/starpoint/personalservice/CdnAssetInstaller.java'),
    (Join-Path $PSScriptRoot 'CdnAssetInstallerRecoveryTest.java')
)
try {
    javac --release 8 -encoding UTF-8 -classpath $AndroidJar -d $CompilationDirectory $Sources
    if ($LASTEXITCODE -ne 0) {
        throw 'CDN 恢复回放编译失败.'
    }
    $Classpath = $CompilationDirectory + [IO.Path]::PathSeparator + $AndroidJar
    java -cp $Classpath dev.starpoint.personalservice.CdnAssetInstallerRecoveryTest
    if ($LASTEXITCODE -ne 0) {
        throw 'CDN 恢复回放失败.'
    }
} finally {
    $ResolvedCompilationDirectory = [IO.Path]::GetFullPath($CompilationDirectory)
    $TemporaryRoot = [IO.Path]::GetFullPath([IO.Path]::GetTempPath()).TrimEnd([IO.Path]::DirectorySeparatorChar)
    if ([IO.Path]::GetDirectoryName($ResolvedCompilationDirectory) -ne $TemporaryRoot) {
        throw '编译目录超出临时目录范围.'
    }
    Remove-Item -LiteralPath $ResolvedCompilationDirectory -Recurse
}
# //// /编译并回放临时目录上的恢复行为 ////
