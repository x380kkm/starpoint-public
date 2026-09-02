# audience: external
# # setup-emulator
# 此脚本安装固定版本的 Android SDK 工具和带 ARM64 native bridge 的 API 35 Google APIs x86_64 镜像, 再创建 24 GB 数据盘的专用 AVD.
# 此脚本需要 JDK 17, Google 官方下载地址可访问, 并且 artifacts 目录可写.

[CmdletBinding()]
param(
    [string]$SdkRoot,
    [string]$AvdHome,
    [string]$JavaHome,
    [string]$AvdName
)

Set-StrictMode -Version Latest
$ErrorActionPreference = "Stop"
Import-Module (Join-Path $PSScriptRoot "protocol-lab.psm1") -Force

$CommandLineToolsUrl = "https://dl.google.com/android/repository/commandlinetools-win-15859902_latest.zip"
$CommandLineToolsSha1 = "b9862337a13e2809a5159dc3a08d058091bd59f6"
$ApiLevel = 35
$Architecture = "x86_64"
$BuildToolsVersion = "35.0.0"
$DataPartitionSize = "24G"
if ([string]::IsNullOrWhiteSpace($AvdName)) {
    $AvdName = "starpoint-cn-api35-x86_64"
}
$SystemImagePackage = "system-images;android-$ApiLevel;google_apis;$Architecture"

# //// 安装并校验 Android command-line tools 22 [@x380kkm 2026-07-20] ////
function Install-AndroidCommandLineTools {
    param(
        [Parameter(Mandatory)]
        [pscustomobject]$Paths
    )

    $SdkManagerPath = Join-Path $Paths.SdkRoot "cmdline-tools\latest\bin\sdkmanager.bat"
    if (Test-Path -LiteralPath $SdkManagerPath -PathType Leaf) {
        return $SdkManagerPath
    }

    New-Item -ItemType Directory -Force -Path $Paths.AndroidArtifactsRoot | Out-Null
    $ArchivePath = Join-Path $Paths.AndroidArtifactsRoot "commandlinetools-win-15859902_latest.zip"
    if (-not (Test-Path -LiteralPath $ArchivePath -PathType Leaf)) {
        Invoke-WebRequest -UseBasicParsing -Uri $CommandLineToolsUrl -OutFile $ArchivePath
    }

    $ActualSha1 = (Get-FileHash -LiteralPath $ArchivePath -Algorithm SHA1).Hash.ToLowerInvariant()
    if ($ActualSha1 -ne $CommandLineToolsSha1) {
        throw "Android command-line tools 校验失败: expected=$CommandLineToolsSha1 actual=$ActualSha1"
    }

    $StagingPath = Join-Path $Paths.AndroidArtifactsRoot ("cmdline-staging-" + [guid]::NewGuid().ToString("N"))
    $TargetPath = Join-Path $Paths.SdkRoot "cmdline-tools\latest"
    if (Test-Path -LiteralPath $TargetPath) {
        throw "Android command-line tools 目标已存在但 sdkmanager 缺失: $TargetPath"
    }

    Expand-Archive -LiteralPath $ArchivePath -DestinationPath $StagingPath
    New-Item -ItemType Directory -Force -Path (Split-Path -Parent $TargetPath) | Out-Null
    Move-Item -LiteralPath (Join-Path $StagingPath "cmdline-tools") -Destination $TargetPath
    Assert-ProtocolLabFile -Path $SdkManagerPath -Description "sdkmanager"
    $SdkManagerPath
}
# //// /安装并校验 Android command-line tools 22 ////

# //// 安装 Emulator, platform-tools, build-tools 和系统镜像 [@x380kkm 2026-07-20] ////
function Install-AndroidPackages {
    param(
        [Parameter(Mandatory)]
        [pscustomobject]$Paths,
        [Parameter(Mandatory)]
        [string]$SdkManagerPath,
        [Parameter(Mandatory)]
        [string]$ImagePackage,
        [Parameter(Mandatory)]
        [string]$ImagePath,
        [Parameter(Mandatory)]
        [string]$BuildToolsPackage,
        [Parameter(Mandatory)]
        [string]$BuildToolsPath
    )

    1..20 | ForEach-Object { "y" } | & $SdkManagerPath --sdk_root=$($Paths.SdkRoot) --licenses | Out-Null
    if ($LASTEXITCODE -ne 0) {
        throw "Android SDK license 接受失败, exit=$LASTEXITCODE"
    }

    & $SdkManagerPath --sdk_root=$($Paths.SdkRoot) "platform-tools" "emulator" $BuildToolsPackage $ImagePackage
    if ($LASTEXITCODE -ne 0) {
        throw "Android SDK 组件安装失败, exit=$LASTEXITCODE"
    }

    Assert-ProtocolLabFile -Path (Join-Path $Paths.SdkRoot "platform-tools\adb.exe") -Description "adb"
    Assert-ProtocolLabFile -Path (Join-Path $Paths.SdkRoot "emulator\emulator.exe") -Description "Android Emulator"
    Assert-ProtocolLabFile -Path (Join-Path $BuildToolsPath "aapt2.exe") -Description "aapt2"
    Assert-ProtocolLabFile -Path (Join-Path $BuildToolsPath "apksigner.bat") -Description "apksigner"
    Assert-ProtocolLabFile -Path $ImagePath -Description "Android system image"
}
# //// /安装 Emulator, platform-tools, build-tools 和系统镜像 ////

# //// 校验 AVD 与请求的 CN 翻译系统镜像一致 [@x380kkm 2026-07-20] ////
function Assert-ProtocolLabAvdConfiguration {
    param(
        [Parameter(Mandatory)]
        [string]$ConfigPath,
        [Parameter(Mandatory)]
        [string]$ImagePackage,
        [Parameter(Mandatory)]
        [string]$Architecture,
        [Parameter(Mandatory)]
        [string]$DataPartitionSize
    )

    $ConfigValues = @{}
    foreach ($Line in Get-Content -LiteralPath $ConfigPath) {
        $Separator = $Line.IndexOf("=")
        if ($Separator -le 0) {
            continue
        }
        $ConfigValues[$Line.Substring(0, $Separator).Trim()] = $Line.Substring($Separator + 1).Trim()
    }

    $ExpectedImageDirectory = $ImagePackage.Replace(";", "\")
    $ActualImageDirectory = if ($ConfigValues.ContainsKey("image.sysdir.1")) {
        $ConfigValues["image.sysdir.1"].Replace("/", "\").TrimEnd([char[]]"\/")
    } else {
        ""
    }
    $Mismatches = @()
    if ($ActualImageDirectory -ne $ExpectedImageDirectory) {
        $Mismatches += "image.sysdir.1=$ActualImageDirectory"
    }
    if (-not $ConfigValues.ContainsKey("abi.type") -or $ConfigValues["abi.type"] -ne $Architecture) {
        $Mismatches += "abi.type=$($ConfigValues['abi.type'])"
    }
    if (-not $ConfigValues.ContainsKey("tag.id") -or $ConfigValues["tag.id"] -ne "google_apis") {
        $Mismatches += "tag.id=$($ConfigValues['tag.id'])"
    }
    if (-not $ConfigValues.ContainsKey("disk.dataPartition.size") -or $ConfigValues["disk.dataPartition.size"] -ne $DataPartitionSize) {
        $Mismatches += "disk.dataPartition.size=$($ConfigValues['disk.dataPartition.size'])"
    }
    if ($Mismatches.Count -gt 0) {
        throw "现有 AVD 与 CN 翻译镜像不一致: $ConfigPath; $($Mismatches -join ', ')"
    }

    $ConfigPath
}
# //// /校验 AVD 与请求的 CN 翻译系统镜像一致 ////

# //// 设置首次启动前的 AVD 数据分区大小 [@x380kkm 2026-07-20] ////
function Set-ProtocolLabAvdDataPartitionSize {
    param(
        [Parameter(Mandatory)]
        [string]$ConfigPath,
        [Parameter(Mandatory)]
        [string]$DataPartitionSize
    )

    $ConfigLines = @(Get-Content -LiteralPath $ConfigPath)
    $SizeWasUpdated = $false
    for ($Index = 0; $Index -lt $ConfigLines.Count; $Index++) {
        if ($ConfigLines[$Index] -match "^disk\.dataPartition\.size=") {
            $ConfigLines[$Index] = "disk.dataPartition.size=$DataPartitionSize"
            $SizeWasUpdated = $true
            break
        }
    }
    if (-not $SizeWasUpdated) {
        $ConfigLines += "disk.dataPartition.size=$DataPartitionSize"
    }
    $ConfigLines | Set-Content -LiteralPath $ConfigPath -Encoding utf8
}
# //// /设置首次启动前的 AVD 数据分区大小 ////

# //// 获取或创建用于协议捕获的 CN native bridge AVD [@x380kkm 2026-07-20] ////
function Get-OrCreateProtocolLabAvd {
    param(
        [Parameter(Mandatory)]
        [pscustomobject]$Paths,
        [Parameter(Mandatory)]
        [string]$Name,
        [Parameter(Mandatory)]
        [string]$ImagePackage,
        [Parameter(Mandatory)]
        [string]$Architecture,
        [Parameter(Mandatory)]
        [string]$DataPartitionSize
    )

    $ConfigPath = Join-Path $Paths.AvdHome "$Name.avd\config.ini"
    if (Test-Path -LiteralPath $ConfigPath -PathType Leaf) {
        return Assert-ProtocolLabAvdConfiguration -ConfigPath $ConfigPath -ImagePackage $ImagePackage -Architecture $Architecture -DataPartitionSize $DataPartitionSize
    }

    New-Item -ItemType Directory -Force -Path $Paths.AvdHome | Out-Null
    $env:ANDROID_AVD_HOME = $Paths.AvdHome
    $AvdManagerPath = Join-Path $Paths.SdkRoot "cmdline-tools\latest\bin\avdmanager.bat"
    Assert-ProtocolLabFile -Path $AvdManagerPath -Description "avdmanager"
    $AvdOutput = @("no" | & $AvdManagerPath create avd --name $Name --package $ImagePackage --device "pixel_6" 2>&1 | ForEach-Object { $_.ToString() })
    $AvdExitCode = $LASTEXITCODE
    if ($AvdExitCode -ne 0) {
        throw "AVD 创建命令失败, exit=$AvdExitCode. $($AvdOutput -join ' ')"
    }
    if (-not (Test-Path -LiteralPath $ConfigPath -PathType Leaf)) {
        throw "AVD 创建失败: $ConfigPath"
    }
    Set-ProtocolLabAvdDataPartitionSize -ConfigPath $ConfigPath -DataPartitionSize $DataPartitionSize
    Assert-ProtocolLabAvdConfiguration -ConfigPath $ConfigPath -ImagePackage $ImagePackage -Architecture $Architecture -DataPartitionSize $DataPartitionSize
}
# //// /获取或创建用于协议捕获的 CN native bridge AVD ////

# //// 完成 Android 模拟器实验环境安装 [@x380kkm 2026-07-20] ////
$Paths = Get-ProtocolLabPaths -SdkRoot $SdkRoot -AvdHome $AvdHome
$ResolvedJavaHome = Resolve-ProtocolLabJavaHome -JavaHome $JavaHome
$env:JAVA_HOME = $ResolvedJavaHome
$env:ANDROID_SDK_ROOT = $Paths.SdkRoot
$SystemImagePath = Join-Path $Paths.SdkRoot "system-images\android-$ApiLevel\google_apis\$Architecture\system.img"
$BuildToolsPackage = "build-tools;$BuildToolsVersion"
$BuildToolsPath = Join-Path $Paths.SdkRoot "build-tools\$BuildToolsVersion"
$SdkManagerPath = Install-AndroidCommandLineTools -Paths $Paths
Install-AndroidPackages -Paths $Paths -SdkManagerPath $SdkManagerPath -ImagePackage $SystemImagePackage -ImagePath $SystemImagePath -BuildToolsPackage $BuildToolsPackage -BuildToolsPath $BuildToolsPath
$ConfigPath = Get-OrCreateProtocolLabAvd -Paths $Paths -Name $AvdName -ImagePackage $SystemImagePackage -Architecture $Architecture -DataPartitionSize $DataPartitionSize

[pscustomobject]@{
    AvdName = $AvdName
    AvdHome = $Paths.AvdHome
    ConfigPath = $ConfigPath
    ApiLevel = $ApiLevel
    Architecture = $Architecture
    BuildToolsVersion = $BuildToolsVersion
    DataPartitionSize = $DataPartitionSize
    JavaHome = $ResolvedJavaHome
    SdkRoot = $Paths.SdkRoot
    SystemImagePackage = $SystemImagePackage
}
# //// /完成 Android 模拟器实验环境安装 ////
