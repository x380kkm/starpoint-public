# audience: internal
# # audit-cn-build-contracts
# 该脚本按构建前顺序编排 CN 契约核查, 解析默认工作区输入, 汇总全部子审计结果, 并在末尾统一返回失败状态.

[CmdletBinding()]
param(
    [string]$RepositoryRoot,
    [string]$WorkspaceRoot,
    [string]$ReferenceRoot,
    [string]$ReferenceSourceRoot,
    [string]$ClientExecutable,
    [string]$CnCdnBundle,
    [string]$CdnRoot,
    [string]$ReferenceCdnRoot,
    [string]$DecompiledRoot,
    [string]$BannerBundleRoot,
    [string]$CnAssetsRoot,
    [string]$ServiceAssetsRoot,
    [string]$AppAssetRoot,
    [string]$CnAppAssetRoot,
    [string]$PhysicsRoot,
    [string]$OutputRoot,
    [string]$SummaryPath,
    [int]$RequestTimeoutMs = 120000,
    [switch]$List,
    [switch]$SelfTestAuditParsing
)

$ErrorActionPreference = 'Stop'
$PSNativeCommandUseErrorActionPreference = $true
Set-StrictMode -Version Latest

$Utf8 = [Text.UTF8Encoding]::new($false)
$ScriptRepositoryRoot = Split-Path -Parent (Split-Path -Parent $PSScriptRoot)
$ScriptWorkspaceRoot = Split-Path -Parent $ScriptRepositoryRoot

# //// 解析显式路径或首个可用默认值 [@x380kkm 2026-08-25] ////
function Resolve-FullPath {
    param([Parameter(Mandatory)][string]$Path)

    [IO.Path]::GetFullPath($Path)
}

function Resolve-PathInput {
    param(
        [Parameter(Mandatory)][string]$Label,
        [AllowNull()][string]$Explicit,
        [Parameter(Mandatory)][string[]]$Candidates
    )

    if (-not [string]::IsNullOrWhiteSpace($Explicit)) {
        $resolved = Resolve-FullPath $Explicit
        return [pscustomobject][ordered]@{
            label = $Label
            path = $resolved
            exists = Test-Path -LiteralPath $resolved
            source = 'explicit'
        }
    }
    foreach ($candidate in $Candidates) {
        if ([string]::IsNullOrWhiteSpace($candidate)) {
            continue
        }
        $resolved = Resolve-FullPath $candidate
        if (Test-Path -LiteralPath $resolved) {
            return [pscustomobject][ordered]@{
                label = $Label
                path = $resolved
                exists = $true
                source = 'default'
            }
        }
    }
    $fallback = Resolve-FullPath $Candidates[0]
    [pscustomobject][ordered]@{
        label = $Label
        path = $fallback
        exists = $false
        source = 'default'
    }
}

function Require-ResolvedPath {
    param([Parameter(Mandatory)][object]$Entry)

    if (-not $Entry.exists) {
        throw "缺少审计输入: $($Entry.label) => $($Entry.path)"
    }
}
# //// /解析显式路径或首个可用默认值 ////

# //// 汇总默认输入并声明每个子审计 [@x380kkm 2026-08-25] ////
function New-BuildContractContext {
    $resolvedRepositoryRoot = Resolve-PathInput -Label 'repository_root' -Explicit $RepositoryRoot -Candidates @($ScriptRepositoryRoot)
    $resolvedWorkspaceRoot = Resolve-PathInput -Label 'workspace_root' -Explicit $WorkspaceRoot -Candidates @($ScriptWorkspaceRoot)
    $resolvedReferenceRoot = Resolve-PathInput -Label 'reference_root' -Explicit $ReferenceRoot -Candidates @(
        (Join-Path $resolvedWorkspaceRoot.path 'startpoint-cn-launcher')
    )
    $resolvedReferenceSourceRoot = Resolve-PathInput -Label 'reference_source_root' -Explicit $ReferenceSourceRoot -Candidates @(
        (Join-Path $resolvedWorkspaceRoot.path 'startpoint-cn')
    )
    $resolvedClientExecutable = Resolve-PathInput -Label 'client_executable' -Explicit $ClientExecutable -Candidates @(
        (Join-Path $resolvedWorkspaceRoot.path 'starpoint/artifacts/ios-device-staging/jp-art-final/Payload/worldflipper.app/worldflipper')
    )
    $resolvedCnCdnBundle = Resolve-PathInput -Label 'cn_cdn_bundle' -Explicit $CnCdnBundle -Candidates @(
        (Join-Path $resolvedWorkspaceRoot.path 'starpoint/artifacts/ios-device-staging/jp-art-final/Payload/worldflipper.app/StarpointCNCDN')
    )
    $resolvedCdnRoot = Resolve-PathInput -Label 'cdn_root' -Explicit $CdnRoot -Candidates @(
        $resolvedCnCdnBundle.path,
        (Join-Path $resolvedWorkspaceRoot.path 'artifacts/cn-cdn/runtime/.cdn/cn'),
        (Join-Path $resolvedWorkspaceRoot.path 'wf-cn-cdn')
    )
    $resolvedReferenceCdnRoot = Resolve-PathInput -Label 'reference_cdn_root' -Explicit $ReferenceCdnRoot -Candidates @(
        (Join-Path $resolvedWorkspaceRoot.path 'artifacts/cn-cdn/runtime/.cdn/cn'),
        $resolvedCnCdnBundle.path,
        (Join-Path $resolvedWorkspaceRoot.path 'wf-cn-cdn')
    )
    $resolvedDecompiledRoot = Resolve-PathInput -Label 'decompiled_root' -Explicit $DecompiledRoot -Candidates @(
        (Join-Path $resolvedWorkspaceRoot.path 'wf-2.1.125-cn-decompiled')
    )
    $resolvedBannerBundleRoot = Resolve-PathInput -Label 'banner_bundle_root' -Explicit $BannerBundleRoot -Candidates @(
        $resolvedCnCdnBundle.path,
        (Join-Path $resolvedWorkspaceRoot.path 'starpoint/.cdn/cn')
    )
    $resolvedCnAssetsRoot = Resolve-PathInput -Label 'cn_assets_root' -Explicit $CnAssetsRoot -Candidates @(
        (Join-Path $resolvedReferenceRoot.path 'resources/server/assets')
    )
    $resolvedServiceAssetsRoot = Resolve-PathInput -Label 'service_assets_root' -Explicit $ServiceAssetsRoot -Candidates @(
        (Join-Path $resolvedRepositoryRoot.path 'assets'),
        (Join-Path $resolvedRepositoryRoot.path 'core/personal-service/assets')
    )
    $resolvedAppAssetRoot = Resolve-PathInput -Label 'app_asset_root' -Explicit $AppAssetRoot -Candidates @(
        (Join-Path $resolvedWorkspaceRoot.path 'starpoint/artifacts/ios-device-staging/jp-art-final/Payload/worldflipper.app/asset')
    )
    $resolvedCnAppAssetRoot = Resolve-PathInput -Label 'cn_app_asset_root' -Explicit $CnAppAssetRoot -Candidates @(
        (Join-Path $resolvedWorkspaceRoot.path 'artifacts/protocol-lab/ios-analysis/ios-1.8.4-5241e51b/unpacked/Payload/worldflipper.app/asset'),
        (Join-Path $resolvedWorkspaceRoot.path 'artifacts/ios-real-app-probe/94771fd/Payload/worldflipper.app/asset')
    )
    $resolvedPhysicsRoot = Resolve-PathInput -Label 'physics_root' -Explicit $PhysicsRoot -Candidates @(
        (Join-Path $resolvedWorkspaceRoot.path 'startpoint-cn'),
        (Join-Path $resolvedDecompiledRoot.path 'scripts/scripts/gacha_physics'),
        (Join-Path $resolvedDecompiledRoot.path 'scripts-priority/scripts/gacha_physics')
    )

    [pscustomobject][ordered]@{
        repository_root = $resolvedRepositoryRoot
        workspace_root = $resolvedWorkspaceRoot
        reference_root = $resolvedReferenceRoot
        reference_source_root = $resolvedReferenceSourceRoot
        client_executable = $resolvedClientExecutable
        cn_cdn_bundle = $resolvedCnCdnBundle
        cdn_root = $resolvedCdnRoot
        reference_cdn_root = $resolvedReferenceCdnRoot
        decompiled_root = $resolvedDecompiledRoot
        banner_bundle_root = $resolvedBannerBundleRoot
        cn_assets_root = $resolvedCnAssetsRoot
        service_assets_root = $resolvedServiceAssetsRoot
        app_asset_root = $resolvedAppAssetRoot
        cn_app_asset_root = $resolvedCnAppAssetRoot
        physics_root = $resolvedPhysicsRoot
    }
}

function New-AuditPlan {
    param(
        [Parameter(Mandatory)][object]$Context,
        [AllowNull()][string]$RunRoot
    )

    $reportRoot = if ([string]::IsNullOrWhiteSpace($RunRoot)) { $null } else { Resolve-FullPath $RunRoot }
    $localTypeScriptCompiler = Join-Path $Context.repository_root.path 'node_modules/typescript/bin/tsc'
    $personalServiceManifest = Join-Path $Context.repository_root.path 'core/personal-service/Cargo.toml'
    $personalServiceProbeName = if ($IsWindows) { 'personal-service-probe.exe' } else { 'personal-service-probe' }
    $personalServiceProbe = Join-Path $Context.repository_root.path "core/personal-service/target/debug/$personalServiceProbeName"
    $singleBattleAssetRoot = Resolve-FullPath (Join-Path $Context.repository_root.path '../startpoint-cn/assets')
    @(
        [pscustomobject][ordered]@{
            id = 'response-contracts'
            name = 'audit-cn-response-contracts'
            runner = 'pwsh'
            file = Join-Path $PSScriptRoot 'audit-cn-response-contracts.ps1'
            arguments = @('-NoProfile', '-File', (Join-Path $PSScriptRoot 'audit-cn-response-contracts.ps1'))
            depends_on = @()
            expects_json = $false
            working_directory = $Context.repository_root.path
            report_path = $null
        },
        [pscustomobject][ordered]@{
            id = 'reference-complete'
            name = 'audit-cn-reference-complete'
            runner = 'pwsh'
            file = Join-Path $PSScriptRoot 'audit-cn-reference-complete.ps1'
            arguments = @(
                '-NoProfile',
                '-File', (Join-Path $PSScriptRoot 'audit-cn-reference-complete.ps1'),
                '-ReferenceRoot', $Context.reference_root.path,
                '-ReferenceSourceRoot', $Context.reference_source_root.path,
                '-ClientExecutable', $Context.client_executable.path,
                '-CnCdnBundle', $Context.cn_cdn_bundle.path,
                '-DecompiledRoot', $Context.decompiled_root.path,
                '-RequestTimeoutMs', [string]$RequestTimeoutMs
            )
            depends_on = @()
            expects_json = $true
            working_directory = $Context.repository_root.path
            report_path = $null
        },
        [pscustomobject][ordered]@{
            id = 'battle-progress'
            name = 'audit-cn-battle-progress-contract'
            runner = 'node'
            file = Join-Path $PSScriptRoot 'audit-cn-battle-progress-contract.mjs'
            arguments = @(
                (Join-Path $PSScriptRoot 'audit-cn-battle-progress-contract.mjs'),
                '--project-root', $Context.repository_root.path,
                '--reference-root', $Context.reference_root.path
            )
            depends_on = @()
            expects_json = $true
            working_directory = $Context.repository_root.path
            report_path = $null
        },
        [pscustomobject][ordered]@{
            id = 'multiplayer-reference'
            name = 'audit-multiplayer-reference-differential'
            runner = 'node'
            file = Join-Path $PSScriptRoot 'audit-multiplayer-reference-differential.mjs'
            arguments = @(
                (Join-Path $PSScriptRoot 'audit-multiplayer-reference-differential.mjs'),
                '--reference-root', (Join-Path $Context.reference_root.path 'resources/server'),
                '--repository-root', $Context.repository_root.path,
                '--summary',
                '--fail-on-differences'
            )
            depends_on = @()
            expects_json = $true
            working_directory = $Context.repository_root.path
            report_path = $null
        },
        [pscustomobject][ordered]@{
            id = 'orderedmap-assets'
            name = 'test-cn-orderedmap'
            runner = 'node'
            file = Join-Path $PSScriptRoot 'test-cn-orderedmap.mjs'
            arguments = @((Join-Path $PSScriptRoot 'test-cn-orderedmap.mjs'))
            depends_on = @()
            expects_json = $false
            working_directory = $Context.repository_root.path
            report_path = $null
            precheck_paths = @(
                [ordered]@{ label = 'cn_gacha_fixture'; path = Join-Path $Context.repository_root.path 'assets/cn_gacha.json' },
                [ordered]@{ label = 'runtime_gacha_fixture'; path = Join-Path $Context.repository_root.path 'assets/gacha.json' },
                [ordered]@{ label = 'gacha_fixture_generator'; path = Join-Path $PSScriptRoot 'generate-cn-gacha-fixture.mjs' }
            )
        },
        [pscustomobject][ordered]@{
            id = 'character-mana-contract'
            name = 'audit-cn-character-mana-contract'
            runner = 'node'
            file = Join-Path $PSScriptRoot 'audit-cn-character-mana-contract.mjs'
            arguments = @((Join-Path $PSScriptRoot 'audit-cn-character-mana-contract.mjs'))
            depends_on = @()
            expects_json = $true
            working_directory = $Context.repository_root.path
            report_path = $null
        },
        [pscustomobject][ordered]@{
            id = 'shop-reward-closure'
            name = 'audit-cn-shop-reward-closure'
            runner = 'node'
            file = Join-Path $PSScriptRoot 'audit-cn-shop-reward-closure.mjs'
            arguments = @((Join-Path $PSScriptRoot 'audit-cn-shop-reward-closure.mjs'))
            depends_on = @()
            expects_json = $true
            working_directory = $Context.repository_root.path
            report_path = $null
        },
        [pscustomobject][ordered]@{
            id = 'activity-catalog'
            name = 'test-cn-activity-catalog'
            runner = 'node'
            file = Join-Path $PSScriptRoot 'test-cn-activity-catalog.mjs'
            arguments = @((Join-Path $PSScriptRoot 'test-cn-activity-catalog.mjs'))
            depends_on = @()
            expects_json = $false
            working_directory = $Context.repository_root.path
            report_path = $null
            precheck_paths = @(
                [ordered]@{ label = 'activity_catalog_generator'; path = Join-Path $PSScriptRoot 'generate-cn-activity-catalog.mjs' },
                [ordered]@{ label = 'activity_master_schema'; path = Join-Path $PSScriptRoot 'cn-activity-master-schema.mjs' }
            )
        },
        [pscustomobject][ordered]@{
            id = 'activity-master-projection'
            name = 'audit-cn-activity-master-projection'
            runner = 'node'
            file = Join-Path $PSScriptRoot 'audit-cn-activity-master-projection.mjs'
            arguments = @((Join-Path $PSScriptRoot 'audit-cn-activity-master-projection.mjs'))
            depends_on = @()
            expects_json = $true
            working_directory = $Context.repository_root.path
            report_path = $null
            precheck_paths = @(
                [ordered]@{ label = 'activity_master_projection'; path = Join-Path $Context.repository_root.path 'core/personal-service/assets/cn-activity-master-projection.json' }
            )
        },
        [pscustomobject][ordered]@{
            id = 'activity-banner-extractor'
            name = 'test-cn-activity-banner-extractor'
            runner = 'pwsh'
            file = Join-Path $PSScriptRoot 'test-cn-activity-banner-extractor.ps1'
            arguments = @('-NoProfile', '-File', (Join-Path $PSScriptRoot 'test-cn-activity-banner-extractor.ps1'))
            depends_on = @()
            expects_json = $false
            working_directory = $Context.repository_root.path
            report_path = $null
            precheck_paths = @(
                [ordered]@{ label = 'activity_banner_extractor'; path = Join-Path $PSScriptRoot 'cn-activity-banner-extractor.psm1' }
            )
        },
        [pscustomobject][ordered]@{
            id = 'activity-controller-ui'
            name = 'personal-service-activity-controller'
            runner = 'node'
            file = Join-Path $Context.repository_root.path 'core/personal-service/web/management/activity-controller.test.mjs'
            arguments = @((Join-Path $Context.repository_root.path 'core/personal-service/web/management/activity-controller.test.mjs'))
            depends_on = @()
            expects_json = $false
            working_directory = $Context.repository_root.path
            report_path = $null
            precheck_paths = @(
                [ordered]@{ label = 'activity_controller_source'; path = Join-Path $Context.repository_root.path 'core/personal-service/web/management/activity-controller.js' }
            )
        },
        [pscustomobject][ordered]@{
            id = 'activity-views-ui'
            name = 'personal-service-activity-views'
            runner = 'node'
            file = Join-Path $Context.repository_root.path 'core/personal-service/web/management/activity-views.test.mjs'
            arguments = @((Join-Path $Context.repository_root.path 'core/personal-service/web/management/activity-views.test.mjs'))
            depends_on = @()
            expects_json = $false
            working_directory = $Context.repository_root.path
            report_path = $null
            precheck_paths = @(
                [ordered]@{ label = 'activity_views_source'; path = Join-Path $Context.repository_root.path 'core/personal-service/web/management/activity-views.js' }
            )
        },
        [pscustomobject][ordered]@{
            id = 'single-battle-fixture'
            name = 'test-cn-single-battle-fixture'
            runner = 'node'
            file = Join-Path $PSScriptRoot 'test-cn-single-battle-fixture.mjs'
            arguments = @((Join-Path $PSScriptRoot 'test-cn-single-battle-fixture.mjs'))
            depends_on = @()
            expects_json = $false
            working_directory = $Context.repository_root.path
            report_path = $null
            precheck_paths = @(
                [ordered]@{ label = 'single_battle_reference_assets'; path = $singleBattleAssetRoot },
                [ordered]@{ label = 'single_battle_fixture_generator'; path = Join-Path $PSScriptRoot 'generate-cn-single-battle-fixture.mjs' }
            )
        },
        [pscustomobject][ordered]@{
            id = 'protocol-coverage'
            name = 'test-cn-protocol-coverage'
            runner = 'node'
            file = Join-Path $PSScriptRoot 'test-cn-protocol-coverage.mjs'
            arguments = @((Join-Path $PSScriptRoot 'test-cn-protocol-coverage.mjs'))
            depends_on = @()
            expects_json = $false
            working_directory = $Context.repository_root.path
            report_path = $null
            precheck_paths = @(
                [ordered]@{ label = 'cn_protocol_coverage'; path = Join-Path $PSScriptRoot 'cn-protocol-coverage.json' }
            )
        },
        [pscustomobject][ordered]@{
            id = 'ios-personal-service-bootstrap-contract'
            name = 'test-ios-personal-service-bootstrap-contract'
            runner = 'uv'
            file = Join-Path $PSScriptRoot 'test_ios_personal_service_bootstrap_contract.py'
            arguments = @(
                'run',
                '--python', '3.12',
                'python', (Join-Path $PSScriptRoot 'test_ios_personal_service_bootstrap_contract.py')
            )
            depends_on = @()
            expects_json = $false
            working_directory = $Context.repository_root.path
            report_path = $null
            precheck_paths = @(
                [ordered]@{
                    label = 'ios_personal_service_bootstrap'
                    path = Join-Path $Context.repository_root.path 'platforms/ios/PersonalServiceBootstrap/StarpointPersonalServiceBootstrap.m'
                },
                [ordered]@{ label = 'ios_framework_build_script'; path = Join-Path $Context.repository_root.path 'platforms/ios/build-framework.sh' },
                [ordered]@{ label = 'ios_package_module'; path = Join-Path $PSScriptRoot 'package_ios_personal_service.py' }
            )
        },
        [pscustomobject][ordered]@{
            id = 'ios-personal-service-package'
            name = 'test-package-ios-personal-service'
            runner = 'uv'
            file = Join-Path $PSScriptRoot 'test_package_ios_personal_service.py'
            arguments = @(
                'run',
                '--python', '3.12',
                '--with', 'Pillow',
                'python',
                (Join-Path $PSScriptRoot 'test_package_ios_personal_service.py')
            )
            depends_on = @()
            expects_json = $false
            working_directory = $Context.repository_root.path
            report_path = $null
            precheck_paths = @(
                [ordered]@{ label = 'ios_package_module'; path = Join-Path $PSScriptRoot 'package_ios_personal_service.py' },
                [ordered]@{ label = 'ios_inventory_module'; path = Join-Path $PSScriptRoot 'ios_inventory.py' },
                [ordered]@{ label = 'ios_cn_aot_patch'; path = Join-Path $PSScriptRoot 'ios_cn_aot_patch.py' },
                [ordered]@{ label = 'ios_cn_compatibility_patch'; path = Join-Path $PSScriptRoot 'ios_cn_compatibility_patch.py' }
            )
        },
        [pscustomobject][ordered]@{
            id = 'ios-cn-game-scenario'
            name = 'test-run-ios-cn-game-scenarios'
            runner = 'uv'
            file = Join-Path $PSScriptRoot 'test_run_ios_cn_game_scenarios.py'
            arguments = @('run', '--python', '3.12', 'python', (Join-Path $PSScriptRoot 'test_run_ios_cn_game_scenarios.py'))
            depends_on = @()
            expects_json = $false
            working_directory = $Context.repository_root.path
            report_path = $null
            precheck_paths = @(
                [ordered]@{ label = 'ios_cn_game_scenario_runner'; path = Join-Path $PSScriptRoot 'run-ios-cn-game-scenarios.py' },
                [ordered]@{ label = 'ios_cn_game_scenario_stages'; path = Join-Path $PSScriptRoot 'ios_cn_game_scenario_stages.py' },
                [ordered]@{ label = 'ios_cn_gameplay_scenario_stages'; path = Join-Path $PSScriptRoot 'ios_cn_gameplay_scenario_stages.py' }
            )
        },
        [pscustomobject][ordered]@{
            id = 'ios-simulator-diagnostic-cdn'
            name = 'test-prepare-ios-simulator-diagnostic-cdn'
            runner = 'uv'
            file = Join-Path $PSScriptRoot 'test_prepare_ios_simulator_diagnostic_cdn.py'
            arguments = @('run', '--python', '3.12', 'python', (Join-Path $PSScriptRoot 'test_prepare_ios_simulator_diagnostic_cdn.py'))
            depends_on = @()
            expects_json = $false
            working_directory = $Context.repository_root.path
            report_path = $null
            precheck_paths = @(
                [ordered]@{ label = 'ios_simulator_diagnostic_cdn_generator'; path = Join-Path $PSScriptRoot 'prepare-ios-simulator-diagnostic-cdn.py' }
            )
        },
        [pscustomobject][ordered]@{
            id = 'server-build'
            name = 'typescript-build'
            runner = 'node'
            file = $localTypeScriptCompiler
            arguments = @($localTypeScriptCompiler)
            depends_on = @()
            expects_json = $false
            working_directory = $Context.repository_root.path
            report_path = $null
        },
        [pscustomobject][ordered]@{
            id = 'personal-service-probe-build'
            name = 'personal-service-probe-build'
            runner = 'cargo'
            file = $personalServiceManifest
            arguments = @(
                '+1.78.0',
                'build',
                '--locked',
                '--manifest-path', $personalServiceManifest,
                '--bin', 'personal-service-probe'
            )
            depends_on = @()
            expects_json = $false
            working_directory = $Context.repository_root.path
            report_path = $null
            precheck_paths = @(
                [ordered]@{ label = 'personal_service_lockfile'; path = Join-Path $Context.repository_root.path 'core/personal-service/Cargo.lock' }
            )
        },
        [pscustomobject][ordered]@{
            id = 'gacha-contract'
            name = 'test-cn-gacha'
            runner = 'node'
            file = Join-Path $PSScriptRoot 'test-cn-gacha.mjs'
            arguments = @((Join-Path $PSScriptRoot 'test-cn-gacha.mjs'))
            depends_on = @('server-build')
            expects_json = $false
            working_directory = $Context.repository_root.path
            report_path = $null
            precheck_paths = @(
                [ordered]@{ label = 'loopback_test_services'; path = Join-Path $PSScriptRoot 'loopback-test-services.js' },
                [ordered]@{ label = 'compiled_server_entry'; path = Join-Path $Context.repository_root.path 'out/start.js' }
            )
        },
        [pscustomobject][ordered]@{
            id = 'leiting-response'
            name = 'test-cn-leiting-response'
            runner = 'node'
            file = Join-Path $PSScriptRoot 'test-cn-leiting-response.mjs'
            arguments = @((Join-Path $PSScriptRoot 'test-cn-leiting-response.mjs'))
            depends_on = @('server-build')
            expects_json = $false
            working_directory = $Context.repository_root.path
            report_path = $null
            precheck_paths = @(
                [ordered]@{ label = 'compiled_server_entry'; path = Join-Path $Context.repository_root.path 'out/start.js' }
            )
        },
        [pscustomobject][ordered]@{
            id = 'http-metadata-observer'
            name = 'test-cn-http-metadata-observer'
            runner = 'node'
            file = Join-Path $PSScriptRoot 'test-cn-http-metadata-observer.mjs'
            arguments = @((Join-Path $PSScriptRoot 'test-cn-http-metadata-observer.mjs'))
            depends_on = @('server-build')
            expects_json = $false
            working_directory = $Context.repository_root.path
            report_path = $null
            precheck_paths = @(
                [ordered]@{ label = 'compiled_http_metadata_observer'; path = Join-Path $Context.repository_root.path 'out/control/cnHttpMetadataObserver.js' }
            )
        },
        [pscustomobject][ordered]@{
            id = 'multiplayer-time'
            name = 'test-multiplayer-time'
            runner = 'node'
            file = Join-Path $PSScriptRoot 'test-multiplayer-time.js'
            arguments = @((Join-Path $PSScriptRoot 'test-multiplayer-time.js'))
            depends_on = @('server-build')
            expects_json = $false
            working_directory = $Context.repository_root.path
            report_path = $null
            precheck_paths = @(
                [ordered]@{ label = 'compiled_matchmaking_store'; path = Join-Path $Context.repository_root.path 'out/multiplayer/matchmakingStore.js' },
                [ordered]@{ label = 'compiled_server_time'; path = Join-Path $Context.repository_root.path 'out/utils.js' }
            )
        },
        [pscustomobject][ordered]@{
            id = 'portable-save'
            name = 'test-portable-save'
            runner = 'node'
            file = Join-Path $PSScriptRoot 'test-portable-save.mjs'
            arguments = @((Join-Path $PSScriptRoot 'test-portable-save.mjs'))
            depends_on = @('server-build')
            expects_json = $false
            working_directory = $Context.repository_root.path
            report_path = $null
            precheck_paths = @(
                [ordered]@{ label = 'compiled_portable_save'; path = Join-Path $Context.repository_root.path 'out/games/starpoint/portableSave.js' },
                [ordered]@{ label = 'compiled_portable_player_data'; path = Join-Path $Context.repository_root.path 'out/games/starpoint/portablePlayerData.js' }
            )
        },
        [pscustomobject][ordered]@{
            id = 'portable-save-roundtrip'
            name = 'test-portable-save-roundtrip'
            runner = 'node'
            file = Join-Path $PSScriptRoot 'test-portable-save-roundtrip.mjs'
            arguments = @((Join-Path $PSScriptRoot 'test-portable-save-roundtrip.mjs'))
            depends_on = @('server-build')
            expects_json = $false
            working_directory = $Context.repository_root.path
            report_path = $null
            precheck_paths = @(
                [ordered]@{ label = 'compiled_portable_save'; path = Join-Path $Context.repository_root.path 'out/games/starpoint/portableSave.js' },
                [ordered]@{ label = 'personal_service_manifest'; path = $personalServiceManifest },
                [ordered]@{ label = 'personal_service_lockfile'; path = Join-Path $Context.repository_root.path 'core/personal-service/Cargo.lock' }
            )
        },
        [pscustomobject][ordered]@{
            id = 'management-api'
            name = 'test-management'
            runner = 'node'
            file = Join-Path $PSScriptRoot 'test-management.js'
            arguments = @((Join-Path $PSScriptRoot 'test-management.js'))
            depends_on = @('server-build')
            expects_json = $false
            working_directory = $Context.repository_root.path
            report_path = $null
            precheck_paths = @(
                [ordered]@{ label = 'compiled_management_api'; path = Join-Path $Context.repository_root.path 'out/routes/management.js' },
                [ordered]@{ label = 'management_page'; path = Join-Path $Context.repository_root.path 'web/pages/management.html' }
            )
        },
        [pscustomobject][ordered]@{
            id = 'management-personal-service-save-sync'
            name = 'test-management-personal-service-save-sync'
            runner = 'node'
            file = Join-Path $PSScriptRoot 'test-management.js'
            arguments = @(
                (Join-Path $PSScriptRoot 'test-management.js'),
                '--personal-service-save-sync'
            )
            depends_on = @('server-build', 'personal-service-probe-build')
            expects_json = $false
            working_directory = $Context.repository_root.path
            report_path = $null
            precheck_paths = @(
                [ordered]@{ label = 'compiled_management_api'; path = Join-Path $Context.repository_root.path 'out/routes/management.js' },
                [ordered]@{ label = 'management_page'; path = Join-Path $Context.repository_root.path 'web/pages/management.html' },
                [ordered]@{ label = 'personal_service_probe'; path = $personalServiceProbe },
                [ordered]@{ label = 'loopback_test_services'; path = Join-Path $PSScriptRoot 'loopback-test-services.js' }
            )
        },
        [pscustomobject][ordered]@{
            id = 'server-state-chain'
            name = 'typescript-server-state-chain'
            runner = 'node'
            file = Join-Path $PSScriptRoot 'test-cn-server.js'
            arguments = @((Join-Path $PSScriptRoot 'test-cn-server.js'))
            depends_on = @('server-build')
            expects_json = $false
            working_directory = $Context.repository_root.path
            report_path = $null
        },
        [pscustomobject][ordered]@{
            id = 'personal-service-state-chain'
            name = 'personal-service-state-chain'
            runner = 'cargo'
            file = Join-Path $Context.repository_root.path 'core/personal-service/Cargo.toml'
            arguments = @(
                '+1.78.0',
                'test',
                '--lib',
                '--tests',
                '--manifest-path', (Join-Path $Context.repository_root.path 'core/personal-service/Cargo.toml')
            )
            depends_on = @()
            expects_json = $false
            working_directory = $Context.repository_root.path
            report_path = $null
        },
        [pscustomobject][ordered]@{
            id = 'gacha-visual-bundle'
            name = 'audit-cn-gacha-visual-bundle'
            runner = 'pwsh'
            file = Join-Path $PSScriptRoot 'audit-cn-gacha-visual-bundle.ps1'
            arguments = @(
                '-NoProfile',
                '-File', (Join-Path $PSScriptRoot 'audit-cn-gacha-visual-bundle.ps1'),
                '-CdnRoot', $Context.cdn_root.path,
                '-ReferenceRoot', $Context.reference_cdn_root.path,
                '-CnAssetsRoot', $Context.cn_assets_root.path,
                '-ServiceAssetsRoot', $Context.service_assets_root.path,
                '-AppAssetRoot', $Context.app_asset_root.path,
                '-CnAppAssetRoot', $Context.cn_app_asset_root.path,
                '-PhysicsRoot', $Context.physics_root.path,
                '-ReportPath', (Join-Path $reportRoot 'gacha-visual-bundle/report.json')
            )
            depends_on = @()
            expects_json = $true
            working_directory = $Context.repository_root.path
            report_path = if ($null -eq $reportRoot) { $null } else { Join-Path $reportRoot 'gacha-visual-bundle/report.json' }
        },
        [pscustomobject][ordered]@{
            id = 'gacha-region-policy'
            name = 'test-cn-gacha-region-policy'
            runner = 'node'
            file = Join-Path $PSScriptRoot 'test-cn-gacha-region-policy.mjs'
            arguments = @((Join-Path $PSScriptRoot 'test-cn-gacha-region-policy.mjs'))
            depends_on = @()
            expects_json = $true
            working_directory = $Context.repository_root.path
            report_path = $null
        },
        [pscustomobject][ordered]@{
            id = 'equipment-gacha-assets'
            name = 'audit-cn-equipment-gacha-assets'
            runner = 'node'
            file = Join-Path $PSScriptRoot 'audit-cn-equipment-gacha-assets.mjs'
            arguments = @(
                (Join-Path $PSScriptRoot 'audit-cn-equipment-gacha-assets.mjs'),
                '--cdn-root', $Context.cdn_root.path,
                '--banner-bundle', $Context.banner_bundle_root.path
            )
            depends_on = @()
            expects_json = $true
            working_directory = $Context.repository_root.path
            report_path = $null
        }
    )
}
# //// /汇总默认输入并声明每个子审计 ////

# //// 执行子进程并记录 UTF-8 输出 [@x380kkm 2026-08-25] ////
function Get-AuditPrecheck {
    param(
        [Parameter(Mandatory)][object]$Audit,
        [Parameter(Mandatory)][object]$Context
    )

    $precheckPathsProperty = $Audit.PSObject.Properties['precheck_paths']
    if ($null -ne $precheckPathsProperty) {
        $checks = @($precheckPathsProperty.Value | ForEach-Object {
            [ordered]@{
                label = $_.label
                path = $_.path
                exists = Test-Path -LiteralPath $_.path
            }
        })
        return [ordered]@{
            kind = 'path-exists'
            ok = @($checks | Where-Object { -not $_.exists }).Count -eq 0
            checks = $checks
        }
    }
    if ($Audit.id -ceq 'server-build') {
        return [ordered]@{
            kind = 'path-exists'
            ok = Test-Path -LiteralPath $Audit.file
            checks = @(
                [ordered]@{
                    label = 'local_typescript_compiler'
                    path = $Audit.file
                    exists = Test-Path -LiteralPath $Audit.file
                }
            )
        }
    }
    if ($Audit.id -cne 'gacha-visual-bundle') {
        return $null
    }
    $checks = @(
        [ordered]@{ label = 'cdn_root'; path = $Context.cdn_root.path },
        [ordered]@{ label = 'reference_cdn_root'; path = $Context.reference_cdn_root.path },
        [ordered]@{ label = 'cn_assets_root'; path = $Context.cn_assets_root.path },
        [ordered]@{ label = 'service_assets_root'; path = $Context.service_assets_root.path },
        [ordered]@{ label = 'app_asset_root'; path = $Context.app_asset_root.path },
        [ordered]@{ label = 'cn_app_asset_root'; path = $Context.cn_app_asset_root.path },
        [ordered]@{ label = 'physics_root'; path = $Context.physics_root.path }
    ) | ForEach-Object {
        [ordered]@{
            label = $_.label
            path = $_.path
            exists = Test-Path -LiteralPath $_.path
        }
    }
    [ordered]@{
        kind = 'path-exists'
        ok = @($checks | Where-Object { -not $_.exists }).Count -eq 0
        checks = $checks
    }
}

function Get-OptionalValue {
    param(
        [AllowNull()][object]$Value,
        [Parameter(Mandatory)][string]$Name
    )

    if ($null -eq $Value) {
        return $null
    }
    if ($Value -is [System.Collections.IDictionary]) {
        if ($Value.Contains($Name)) {
            return $Value[$Name]
        }
        return $null
    }
    $property = $Value.PSObject.Properties[$Name]
    if ($null -eq $property) {
        return $null
    }
    $property.Value
}

function Get-NamedEntries {
    param([AllowNull()][object]$Value)

    if ($null -eq $Value) {
        return @()
    }
    if ($Value -is [System.Collections.IDictionary]) {
        return @($Value.Keys | ForEach-Object {
            [ordered]@{
                name = [string]$_
                value = $Value[$_]
            }
        })
    }
    return @($Value.PSObject.Properties | ForEach-Object {
        [ordered]@{
            name = $_.Name
            value = $_.Value
        }
    })
}

function ConvertFrom-AuditJsonText {
    param([Parameter(Mandatory)][string]$Text)

    $trimmed = $Text.Trim()
    if ([string]::IsNullOrWhiteSpace($trimmed)) {
        return $null
    }
    try {
        return [pscustomobject][ordered]@{
            parsed = $trimmed | ConvertFrom-Json -Depth 100
            text = $trimmed
        }
    } catch {
    }

    $startMatches = [regex]::Matches($trimmed, '(?m)^[\{\[]')
    for ($index = $startMatches.Count - 1; $index -ge 0; $index -= 1) {
        $candidate = $trimmed.Substring($startMatches[$index].Index).Trim()
        try {
            return [pscustomobject][ordered]@{
                parsed = $candidate | ConvertFrom-Json -Depth 100
                text = $candidate
            }
        } catch {
        }
    }
    $fallbackStarts = @()
    for ($index = $trimmed.Length - 1; $index -ge 0; $index -= 1) {
        $character = $trimmed[$index]
        if ($character -ceq '{' -or $character -ceq '[') {
            $fallbackStarts += $index
        }
    }
    foreach ($startIndex in $fallbackStarts) {
        $candidate = $trimmed.Substring($startIndex).Trim()
        try {
            return [pscustomobject][ordered]@{
                parsed = $candidate | ConvertFrom-Json -Depth 100
                text = $candidate
            }
        } catch {
        }
    }
    return $null
}

function Invoke-AuditParsingSelfTest {
    $fixtureRoot = Join-Path ([IO.Path]::GetTempPath()) ('starpoint-build-contracts-selftest-' + [Guid]::NewGuid().ToString('N'))
    New-Item -ItemType Directory -Force -Path $fixtureRoot | Out-Null
    try {
        $stdoutPath = Join-Path $fixtureRoot 'stdout.txt'
        $jsonPath = Join-Path $fixtureRoot 'stdout.json'
        $reportPath = Join-Path $fixtureRoot 'report.json'
        $noiseText = @(
            'ordinary output line 1',
            'ordinary output line 2',
            '{',
            '  "summary": {',
            '    "total": 1',
            '  },',
            '  "reports": null',
            '}'
        ) -join "`n"
        $noiseResolved = ConvertFrom-AuditJsonText -Text $noiseText
        $caseWithoutPrimary = Get-AuditReports ([pscustomobject][ordered]@{
            id = 'reference-complete'
            parsed_json = [pscustomobject][ordered]@{
                reports = [pscustomobject][ordered]@{
                    complete = 'complete-audit.json'
                    summary = 'complete-audit-summary.json'
                }
            }
            report_path = $null
            stdout_path = $stdoutPath
        })
        $caseNullReports = Get-AuditReports ([pscustomobject][ordered]@{
            id = 'reference-complete'
            parsed_json = [pscustomobject][ordered]@{
                reports = $null
            }
            report_path = $reportPath
            stdout_path = $stdoutPath
        })
        $caseTrailingJson = Get-AuditReports ([pscustomobject][ordered]@{
            id = 'reference-complete'
            parsed_json = $noiseResolved.parsed
            report_path = $null
            stdout_path = $stdoutPath
        })
        $checks = @(
            [ordered]@{
                name = 'reports-without-primary'
                ok = $caseWithoutPrimary.primary -ceq $stdoutPath -and
                    $caseWithoutPrimary.summary -ceq 'complete-audit-summary.json' -and
                    $caseWithoutPrimary.complete -ceq 'complete-audit.json'
            },
            [ordered]@{
                name = 'null-reports-falls-back-to-report-path'
                ok = $caseNullReports.primary -ceq $reportPath
            },
            [ordered]@{
                name = 'trailing-json-parsed-after-ordinary-output'
                ok = $null -ne $noiseResolved -and $caseTrailingJson.primary -ceq $stdoutPath
            }
        )
        $result = [ordered]@{
            status = if (@($checks | Where-Object { -not $_.ok }).Count -eq 0) { 'passed' } else { 'failed' }
            checks = $checks
            reports_without_primary = $caseWithoutPrimary
            reports_null = $caseNullReports
            trailing_json = [ordered]@{
                parsed = $null -ne $noiseResolved
                reports = $caseTrailingJson
            }
        }
        $result | ConvertTo-Json -Depth 20
        if ($result.status -cne 'passed') {
            exit 1
        }
        return
    } finally {
        if (Test-Path -LiteralPath $fixtureRoot) {
            Remove-Item -LiteralPath $fixtureRoot -Recurse -Force
        }
    }
}

function Invoke-AuditProcess {
    param(
        [Parameter(Mandatory)][object]$Audit,
        [Parameter(Mandatory)][string]$AuditDirectory
    )

    $stdoutPath = Join-Path $AuditDirectory 'stdout.txt'
    $stderrPath = Join-Path $AuditDirectory 'stderr.txt'
    $stdoutJsonPath = Join-Path $AuditDirectory 'stdout.json'

    New-Item -ItemType Directory -Force -Path $AuditDirectory | Out-Null
    $startInfo = [System.Diagnostics.ProcessStartInfo]::new()
    $startInfo.FileName = $Audit.runner
    $startInfo.WorkingDirectory = $Audit.working_directory
    $startInfo.RedirectStandardOutput = $true
    $startInfo.RedirectStandardError = $true
    $startInfo.UseShellExecute = $false
    $startInfo.StandardOutputEncoding = $Utf8
    $startInfo.StandardErrorEncoding = $Utf8
    foreach ($argument in @($Audit.arguments)) {
        [void]$startInfo.ArgumentList.Add([string]$argument)
    }

    $stopwatch = [System.Diagnostics.Stopwatch]::StartNew()
    $process = [System.Diagnostics.Process]::new()
    $process.StartInfo = $startInfo
    [void]$process.Start()
    $stdoutTask = $process.StandardOutput.ReadToEndAsync()
    $stderrTask = $process.StandardError.ReadToEndAsync()
    $process.WaitForExit()
    $stdoutTask.Wait()
    $stderrTask.Wait()
    $stopwatch.Stop()

    $stdout = $stdoutTask.Result
    $stderr = $stderrTask.Result
    [IO.File]::WriteAllText($stdoutPath, $stdout, $Utf8)
    [IO.File]::WriteAllText($stderrPath, $stderr, $Utf8)

    $parsedJson = $null
    $jsonPath = $null
    if ($Audit.expects_json -and -not [string]::IsNullOrWhiteSpace($stdout)) {
        $resolvedJson = ConvertFrom-AuditJsonText -Text $stdout
        if ($null -eq $resolvedJson) {
            if ($process.ExitCode -eq 0) {
                throw "审计 JSON 无法解析: $($Audit.name)"
            }
        } else {
            $parsedJson = $resolvedJson.parsed
            $normalizedJson = $parsedJson | ConvertTo-Json -Depth 100
            [IO.File]::WriteAllText($stdoutJsonPath, $normalizedJson, $Utf8)
            $jsonPath = $stdoutJsonPath
        }
    }

    [pscustomobject][ordered]@{
        id = $Audit.id
        name = $Audit.name
        duration_ms = [int][Math]::Round($stopwatch.Elapsed.TotalMilliseconds)
        exit_code = $process.ExitCode
        stdout_path = $stdoutPath
        stderr_path = $stderrPath
        json_path = $jsonPath
        report_path = $Audit.report_path
        parsed_json = $parsedJson
    }
}

function Get-AuditReports {
    param([Parameter(Mandatory)][object]$AuditResult)

    $reports = [ordered]@{}
    $reportsValue = $null
    if ($AuditResult.id -ceq 'reference-complete' -and $null -ne $AuditResult.parsed_json) {
        $reportsValue = Get-OptionalValue -Value $AuditResult.parsed_json -Name 'reports'
    }
    foreach ($entry in @(Get-NamedEntries -Value $reportsValue)) {
        $reports[$entry.name] = $entry.value
    }
    $primary = Get-OptionalValue -Value $reportsValue -Name 'primary'
    if ([string]::IsNullOrWhiteSpace([string]$primary)) {
        $primary = if (-not [string]::IsNullOrWhiteSpace($AuditResult.report_path)) {
            $AuditResult.report_path
        } else {
            $AuditResult.stdout_path
        }
    }
    $reports.primary = $primary
    $reports
}

function New-AuditFailureResult {
    param(
        [Parameter(Mandatory)][string]$AuditId,
        [Parameter(Mandatory)][string]$AuditName,
        [Parameter(Mandatory)][string]$StdoutPath,
        [Parameter(Mandatory)][string]$StderrPath,
        [AllowNull()][string]$JsonPath,
        [AllowNull()][string]$ReportPath,
        [Parameter(Mandatory)][int]$ExitCode,
        [Parameter(Mandatory)][string]$Message,
        [int]$DurationMs = 0
    )

    $reports = if ([string]::IsNullOrWhiteSpace($ReportPath)) {
        if ([string]::IsNullOrWhiteSpace($JsonPath)) {
            [ordered]@{ primary = $StdoutPath }
        } else {
            [ordered]@{ primary = $JsonPath }
        }
    } else {
        [ordered]@{ primary = $ReportPath }
    }
    [ordered]@{
        id = $AuditId
        name = $AuditName
        status = 'failed'
        duration_ms = $DurationMs
        exit_code = $ExitCode
        stdout_path = $StdoutPath
        stderr_path = $StderrPath
        json_path = $JsonPath
        report_path = $reports.primary
        reports = $reports
        error = $Message
    }
}

function New-AuditSkippedResult {
    param(
        [Parameter(Mandatory)][string]$AuditId,
        [Parameter(Mandatory)][string]$AuditName,
        [Parameter(Mandatory)][string]$StdoutPath,
        [Parameter(Mandatory)][string]$StderrPath,
        [AllowNull()][string]$ReportPath,
        [Parameter(Mandatory)][string]$Dependency,
        [Parameter(Mandatory)][string]$Reason
    )

    New-Item -ItemType Directory -Force -Path (Split-Path -Parent $StdoutPath) | Out-Null
    [IO.File]::WriteAllText($StdoutPath, '', $Utf8)
    [IO.File]::WriteAllText($StderrPath, '', $Utf8)
    $reports = if ([string]::IsNullOrWhiteSpace($ReportPath)) {
        [ordered]@{ primary = $StdoutPath }
    } else {
        [ordered]@{ primary = $ReportPath }
    }
    [ordered]@{
        id = $AuditId
        name = $AuditName
        status = 'skipped'
        duration_ms = 0
        exit_code = $null
        stdout_path = $StdoutPath
        stderr_path = $StderrPath
        json_path = $null
        report_path = $reports.primary
        reports = $reports
        reason = $Reason
        dependency = $Dependency
    }
}

function Write-BuildSummary {
    param(
        [Parameter(Mandatory)][object]$Summary,
        [Parameter(Mandatory)][string]$Path
    )

    $directory = Split-Path -Parent $Path
    if (-not [string]::IsNullOrWhiteSpace($directory)) {
        New-Item -ItemType Directory -Force -Path $directory | Out-Null
    }
    [IO.File]::WriteAllText($Path, ($Summary | ConvertTo-Json -Depth 50), $Utf8)
}
# //// /执行子进程并记录 UTF-8 输出 ////

# //// 列出或执行构建前契约核查 [@x380kkm 2026-08-25] ////
$context = New-BuildContractContext
$pathEntries = @(
    $context.repository_root,
    $context.workspace_root,
    $context.reference_root,
    $context.reference_source_root,
    $context.client_executable,
    $context.cn_cdn_bundle,
    $context.cdn_root,
    $context.reference_cdn_root,
    $context.decompiled_root,
    $context.banner_bundle_root,
    $context.cn_assets_root,
    $context.service_assets_root,
    $context.app_asset_root,
    $context.cn_app_asset_root,
    $context.physics_root
)
$plannedRunRoot = if ([string]::IsNullOrWhiteSpace($OutputRoot)) {
    Join-Path ([IO.Path]::GetTempPath()) ('starpoint-build-contracts-' + [Guid]::NewGuid().ToString('N'))
} else {
    Resolve-FullPath $OutputRoot
}
$plannedSummaryPath = if ([string]::IsNullOrWhiteSpace($SummaryPath)) {
    Join-Path $plannedRunRoot 'build-contract-summary.json'
} else {
    Resolve-FullPath $SummaryPath
}

if ($SelfTestAuditParsing) {
    Invoke-AuditParsingSelfTest
    return
}

$auditPlan = @(New-AuditPlan -Context $context -RunRoot $plannedRunRoot)

if ($List) {
    $plan = [ordered]@{
        status = 'planned'
        repository_root = $context.repository_root.path
        workspace_root = $context.workspace_root.path
        summary_path = $plannedSummaryPath
        paths = $pathEntries
        audits = @($auditPlan | ForEach-Object {
            [ordered]@{
                id = $_.id
                name = $_.name
                runner = $_.runner
                file = $_.file
                arguments = $_.arguments
                working_directory = $_.working_directory
                report_path = $_.report_path
                depends_on = @($_.depends_on)
                expects_json = $_.expects_json
                precheck = Get-AuditPrecheck -Audit $_ -Context $context
            }
        })
    }
    $plan | ConvertTo-Json -Depth 30
    return
}

foreach ($entry in $pathEntries) {
    Require-ResolvedPath $entry
}
foreach ($audit in $auditPlan) {
    if (-not (Test-Path -LiteralPath $audit.file)) {
        throw "缺少子审计脚本: $($audit.file)"
    }
}

New-Item -ItemType Directory -Force -Path $plannedRunRoot | Out-Null
$results = @()
$failureExitCode = 0
$resultByAuditId = @{}

foreach ($audit in $auditPlan) {
    $auditDirectory = Join-Path $plannedRunRoot $audit.id
    $blockedDependency = @($audit.depends_on | Where-Object {
        $dependencyResult = $resultByAuditId[$_]
        $null -ne $dependencyResult -and
            $dependencyResult.status -notin @('passed', 'warning')
    } | Select-Object -First 1)
    if ($blockedDependency.Count -gt 0) {
        $skippedResult = New-AuditSkippedResult `
            -AuditId $audit.id `
            -AuditName $audit.name `
            -StdoutPath (Join-Path $auditDirectory 'stdout.txt') `
            -StderrPath (Join-Path $auditDirectory 'stderr.txt') `
            -ReportPath $audit.report_path `
            -Dependency $blockedDependency[0] `
            -Reason 'dependency'
        $results += $skippedResult
        $resultByAuditId[$audit.id] = $skippedResult
        continue
    }
    try {
        $auditResult = Invoke-AuditProcess -Audit $audit -AuditDirectory $auditDirectory
    } catch {
        $failureResult = New-AuditFailureResult `
            -AuditId $audit.id `
            -AuditName $audit.name `
            -StdoutPath (Join-Path $auditDirectory 'stdout.txt') `
            -StderrPath (Join-Path $auditDirectory 'stderr.txt') `
            -JsonPath $null `
            -ReportPath $audit.report_path `
            -ExitCode 1 `
            -Message $_.Exception.Message
        $results += $failureResult
        $resultByAuditId[$audit.id] = $failureResult
        if ($failureExitCode -eq 0) {
            $failureExitCode = 1
        }
        continue
    }
    if ($audit.expects_json -and $null -eq $auditResult.parsed_json -and $auditResult.exit_code -eq 0) {
        $failureResult = New-AuditFailureResult `
            -AuditId $auditResult.id `
            -AuditName $auditResult.name `
            -StdoutPath $auditResult.stdout_path `
            -StderrPath $auditResult.stderr_path `
            -JsonPath $null `
            -ReportPath $auditResult.report_path `
            -ExitCode 1 `
            -Message "审计没有输出 JSON: $($auditResult.name)" `
            -DurationMs $auditResult.duration_ms
        $results += $failureResult
        $resultByAuditId[$audit.id] = $failureResult
        if ($failureExitCode -eq 0) {
            $failureExitCode = 1
        }
        continue
    }
    $reports = Get-AuditReports -AuditResult $auditResult
    $reportedAuditStatus = [string](Get-OptionalValue -Value $auditResult.parsed_json -Name 'audit_status')
    $resultStatus = if ($auditResult.exit_code -ne 0) {
        'failed'
    } elseif ($reportedAuditStatus -ceq 'warning') {
        'warning'
    } else {
        'passed'
    }
    $resultEntry = [ordered]@{
        id = $auditResult.id
        name = $auditResult.name
        status = $resultStatus
        duration_ms = $auditResult.duration_ms
        exit_code = $auditResult.exit_code
        stdout_path = $auditResult.stdout_path
        stderr_path = $auditResult.stderr_path
        json_path = $auditResult.json_path
        report_path = $reports.primary
        reports = $reports
    }
    $results += $resultEntry
    $resultByAuditId[$audit.id] = $resultEntry

    if ($auditResult.exit_code -ne 0) {
        if ($failureExitCode -eq 0) {
            $failureExitCode = $auditResult.exit_code
        }
    }
}

$failedAudits = @($results | Where-Object status -ceq 'failed')
$passedAudits = @($results | Where-Object status -ceq 'passed')
$warningAudits = @($results | Where-Object status -ceq 'warning')
$skippedAudits = @($results | Where-Object status -ceq 'skipped')

$summary = [ordered]@{
    status = if ($failureExitCode -ne 0) {
        'failed'
    } elseif ($warningAudits.Count -gt 0) {
        'passed_with_warnings'
    } else {
        'passed'
    }
    repository_root = $context.repository_root.path
    workspace_root = $context.workspace_root.path
    output_root = $plannedRunRoot
    summary_path = $plannedSummaryPath
    audit_count = $results.Count
    passed_count = $passedAudits.Count
    warning_count = $warningAudits.Count
    failed_count = $failedAudits.Count
    skipped_count = $skippedAudits.Count
    failed_audits = @($failedAudits | ForEach-Object {
        [ordered]@{
            id = $_.id
            name = $_.name
            exit_code = $_.exit_code
            report_path = $_.report_path
        }
    })
    warnings = @($warningAudits | ForEach-Object {
        [ordered]@{
            id = $_.id
            name = $_.name
            report_path = $_.report_path
        }
    })
    skipped_audits = @($skippedAudits | ForEach-Object {
        [ordered]@{
            id = $_.id
            name = $_.name
            reason = $_.reason
            dependency = $_.dependency
            report_path = $_.report_path
        }
    })
    paths = $pathEntries
    audits = $results
}
Write-BuildSummary -Summary $summary -Path $plannedSummaryPath
$summary | ConvertTo-Json -Depth 40

if ($failureExitCode -ne 0) {
    exit $failureExitCode
}
# //// /列出或执行构建前契约核查 ////
