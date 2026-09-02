# audience: internal
# # android-packaging-paths
# 此模块解析 Android 打包使用的共享工作区和 CN CDN 根目录, 并核对共享服务输入.

Set-StrictMode -Version Latest
$ErrorActionPreference = "Stop"

# //// 从 Git 公共目录解析共享工作区 [@x380kkm 2026-09-01] ////
function Resolve-AndroidPackagingWorkspaceRoot {
    [CmdletBinding()]
    param([Parameter(Mandatory)][string]$RepositoryRoot)

    $ResolvedRepositoryRoot = [IO.Path]::GetFullPath($RepositoryRoot)
    $GitCommonDirectory = @(
        & git -C $ResolvedRepositoryRoot rev-parse --git-common-dir 2>$null |
            Select-Object -First 1
    )
    $GitCommandSucceeded = $?
    if ($GitCommandSucceeded -and $GitCommonDirectory.Count -eq 1 -and
        -not [string]::IsNullOrWhiteSpace([string]$GitCommonDirectory[0])) {
        $CommonDirectory = [string]$GitCommonDirectory[0]
        if (-not [IO.Path]::IsPathRooted($CommonDirectory)) {
            $CommonDirectory = Join-Path $ResolvedRepositoryRoot $CommonDirectory
        }
        $CommonDirectory = [IO.Path]::GetFullPath($CommonDirectory)
        if ((Split-Path -Leaf $CommonDirectory) -eq ".git") {
            return [IO.Path]::GetFullPath(
                (Split-Path -Parent (Split-Path -Parent $CommonDirectory))
            )
        }
    }

    return [IO.Path]::GetFullPath((Split-Path -Parent $ResolvedRepositoryRoot))
}
# //// /从 Git 公共目录解析共享工作区 ////

# //// 判断 Android CDN 根目录的必要输入 [@x380kkm 2026-09-01] ////
function Test-AndroidPackagingCdnRoot {
    [CmdletBinding()]
    param([Parameter(Mandatory)][string]$Path)

    if (-not (Test-Path -LiteralPath $Path -PathType Container)) {
        return $false
    }
    foreach ($RequiredPath in @(
        "entities/10939-android_medium.csv",
        "entities/PathFile.csv"
    )) {
        if (-not (Test-Path -LiteralPath (Join-Path $Path $RequiredPath) -PathType Leaf)) {
            return $false
        }
    }
    $AndroidArchiveRoot = Join-Path $Path "archive-android-full"
    $ArchiveExists = Test-Path -LiteralPath $AndroidArchiveRoot -PathType Container
    $ArchiveFileCount = if ($ArchiveExists) {
        @(
            Get-ChildItem -LiteralPath $AndroidArchiveRoot -Recurse -File -ErrorAction SilentlyContinue
        ).Count
    } else {
        0
    }
    $ArchiveExists -and $ArchiveFileCount -gt 0
}
# //// /判断 Android CDN 根目录的必要输入 ////

# //// 解析 Android CDN 默认根目录 [@x380kkm 2026-09-01] ////
function Resolve-AndroidPackagingCdnRoot {
    [CmdletBinding()]
    param(
        [Parameter(Mandatory)][string]$RepositoryRoot,
        [Parameter(Mandatory)][string]$WorkspaceRoot,
        [string]$ExplicitPath
    )

    $Candidates = [Collections.Generic.List[string]]::new()
    if (-not [string]::IsNullOrWhiteSpace($ExplicitPath)) {
        $Candidates.Add([IO.Path]::GetFullPath($ExplicitPath))
    } else {
        foreach ($Candidate in @(
            (Join-Path $RepositoryRoot ".cdn/cn"),
            (Join-Path $WorkspaceRoot "starpoint/.cdn/cn"),
            (Join-Path $WorkspaceRoot ".cdn/cn"),
            (Join-Path $WorkspaceRoot "artifacts/cn-cdn/runtime/.cdn/cn")
        )) {
            $Candidates.Add([IO.Path]::GetFullPath($Candidate))
        }
    }

    foreach ($Candidate in ($Candidates | Select-Object -Unique)) {
        if (Test-AndroidPackagingCdnRoot -Path $Candidate) {
            return (Resolve-Path -LiteralPath $Candidate).Path
        }
    }

    throw "未找到包含 Android EntityLists, PathFile 和 archive-android-full 的 CN CDN 根目录: $($Candidates -join '; ')"
}
# //// /解析 Android CDN 默认根目录 ////

# //// 执行 Android 打包所需 Git 查询 [@x380kkm 2026-09-01] ////
function Invoke-AndroidPackagingGit {
    [CmdletBinding()]
    param(
        [Parameter(Mandatory)][string]$RepositoryRoot,
        [Parameter(Mandatory)][string[]]$ArgumentList
    )

    $LASTEXITCODE = 0
    $Output = @(
        & git -C $RepositoryRoot @ArgumentList 2>&1 |
            ForEach-Object { $_.ToString() }
    )
    $ExitCode = $LASTEXITCODE
    if ($ExitCode -ne 0) {
        throw "Android 打包 Git 查询失败: git $($ArgumentList -join ' ')`n$($Output -join "`n")"
    }
    ($Output -join "`n").Trim()
}
# //// /执行 Android 打包所需 Git 查询 ////

# //// 获取工作树中指定路径的 Git 子树 [@x380kkm 2026-09-01] ////
function Get-AndroidWorkingTreePathTree {
    [CmdletBinding()]
    param(
        [Parameter(Mandatory)][string]$RepositoryRoot,
        [Parameter(Mandatory)][string]$RelativePath
    )

    $IndexPath = Join-Path ([IO.Path]::GetTempPath()) (
        "starpoint-android-parity-" + [Guid]::NewGuid().ToString("N") + ".index"
    )
    $PreviousIndexPath = [Environment]::GetEnvironmentVariable("GIT_INDEX_FILE", "Process")
    try {
        $env:GIT_INDEX_FILE = $IndexPath
        $ReadTreeOutput = @(& git -C $RepositoryRoot read-tree HEAD 2>&1)
        if ($LASTEXITCODE -ne 0) {
            throw "Android 工作树一致性核对无法读取 HEAD 索引: $($ReadTreeOutput -join "`n")"
        }
        $AddOutput = @(& git -C $RepositoryRoot add --all -- $RelativePath 2>&1)
        if ($LASTEXITCODE -ne 0) {
            throw "Android 工作树一致性核对无法暂存路径快照: $($AddOutput -join "`n")"
        }
        $TreeOutput = @(& git -C $RepositoryRoot write-tree 2>&1)
        if ($LASTEXITCODE -ne 0 -or $TreeOutput.Count -eq 0) {
            throw "Android 工作树一致性核对无法生成临时树: $($TreeOutput -join "`n")"
        }
        $RootTree = [string]$TreeOutput[-1]
        $PathTreeOutput = @(& git -C $RepositoryRoot rev-parse "$RootTree`:$RelativePath" 2>&1)
        if ($LASTEXITCODE -ne 0 -or $PathTreeOutput.Count -eq 0) {
            throw "Android 工作树一致性核对无法读取路径树: $($PathTreeOutput -join "`n")"
        }
        ([string]$PathTreeOutput[-1]).Trim()
    } finally {
        if ([string]::IsNullOrWhiteSpace($PreviousIndexPath)) {
            Remove-Item Env:GIT_INDEX_FILE -ErrorAction SilentlyContinue
        } else {
            $env:GIT_INDEX_FILE = $PreviousIndexPath
        }
        Remove-Item -LiteralPath $IndexPath -Force -ErrorAction SilentlyContinue
        Remove-Item -LiteralPath "$IndexPath.lock" -Force -ErrorAction SilentlyContinue
    }
}
# //// /获取工作树中指定路径的 Git 子树 ////

# //// 核对 Android 与 iOS tag 的共享路径 [@x380kkm 2026-09-01] ////
function Assert-AndroidPathMatchesBaseline {
    [CmdletBinding()]
    param(
        [Parameter(Mandatory)][string]$RepositoryRoot,
        [Parameter(Mandatory)][string]$BaselineRef,
        [Parameter(Mandatory)][string]$RelativePath,
        [Parameter(Mandatory)][string]$DisplayName
    )

    $SourceTree = Invoke-AndroidPackagingGit `
        -RepositoryRoot $RepositoryRoot `
        -ArgumentList @("rev-parse", "HEAD:$RelativePath")
    $BaselineTree = Invoke-AndroidPackagingGit `
        -RepositoryRoot $RepositoryRoot `
        -ArgumentList @("rev-parse", "$BaselineRef`:$RelativePath")
    $WorkingTree = Get-AndroidWorkingTreePathTree `
        -RepositoryRoot $RepositoryRoot `
        -RelativePath $RelativePath
    $WorkingTreeStatus = Invoke-AndroidPackagingGit `
        -RepositoryRoot $RepositoryRoot `
        -ArgumentList @("status", "--porcelain=v1", "--untracked-files=all", "--", $RelativePath)

    if (-not [string]::IsNullOrWhiteSpace($WorkingTreeStatus)) {
        throw "Android $DisplayName 路径包含未提交改动, 无法确认与 iOS tag 一致:`n$WorkingTreeStatus"
    }
    if ($WorkingTree -cne $SourceTree) {
        throw "Android $DisplayName 工作树与 HEAD 不一致: working=$WorkingTree head=$SourceTree"
    }
    if ($SourceTree -cne $BaselineTree) {
        throw "Android $DisplayName 与 iOS tag 不一致: source=$SourceTree baseline=$BaselineTree ref=$BaselineRef"
    }

    [ordered]@{
        path = $RelativePath
        source_tree = $SourceTree
        baseline_tree = $BaselineTree
        working_tree = $WorkingTree
        working_tree_clean = [string]::IsNullOrWhiteSpace($WorkingTreeStatus)
        consistent = $true
    }
}
# //// /核对 Android 与 iOS tag 的共享路径 ////

# //// 核对 Android 与 iOS tag 的共享服务和资源 [@x380kkm 2026-09-01] ////
function Assert-AndroidPersonalServiceBaseline {
    [CmdletBinding()]
    param(
        [Parameter(Mandatory)][string]$RepositoryRoot,
        [Parameter(Mandatory)][string]$BaselineRef
    )

    $ResolvedRepositoryRoot = [IO.Path]::GetFullPath($RepositoryRoot)
    $SourceCommit = Invoke-AndroidPackagingGit `
        -RepositoryRoot $ResolvedRepositoryRoot `
        -ArgumentList @("rev-parse", "HEAD^{commit}")
    $BaselineCommit = Invoke-AndroidPackagingGit `
        -RepositoryRoot $ResolvedRepositoryRoot `
        -ArgumentList @("rev-parse", "$BaselineRef^{commit}")
    $ServiceParity = Assert-AndroidPathMatchesBaseline `
        -RepositoryRoot $ResolvedRepositoryRoot `
        -BaselineRef $BaselineRef `
        -RelativePath "core/personal-service" `
        -DisplayName "个人服务"
    $AssetsParity = Assert-AndroidPathMatchesBaseline `
        -RepositoryRoot $ResolvedRepositoryRoot `
        -BaselineRef $BaselineRef `
        -RelativePath "assets" `
        -DisplayName "顶层 assets"

    [ordered]@{
        source_commit = $SourceCommit
        service_baseline_ref = $BaselineRef
        service_baseline_commit = $BaselineCommit
        personal_service_tree = $ServiceParity.source_tree
        personal_service_baseline_tree = $ServiceParity.baseline_tree
        personal_service_working_tree = $ServiceParity.working_tree
        assets_tree = $AssetsParity.source_tree
        assets_working_tree = $AssetsParity.working_tree
        assets_baseline_tree = $AssetsParity.baseline_tree
        personal_service_consistent = $ServiceParity.consistent
        assets_consistent = $AssetsParity.consistent
        working_tree_clean = ($ServiceParity.working_tree_clean -and $AssetsParity.working_tree_clean)
        shared_input_consistency = [ordered]@{
            personal_service = $ServiceParity
            assets = $AssetsParity
        }
    }
}
# //// /核对 Android 与 iOS tag 的共享服务和资源 ////

Export-ModuleMember -Function @(
    "Assert-AndroidPersonalServiceBaseline",
    "Resolve-AndroidPackagingWorkspaceRoot",
    "Resolve-AndroidPackagingCdnRoot",
    "Test-AndroidPackagingCdnRoot"
)
