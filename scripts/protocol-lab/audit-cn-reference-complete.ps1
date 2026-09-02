# audience: internal
# # cn-reference-complete-audit
# 此脚本运行参考 CN 服务, 当前 iOS 客户端, 动态行为和非游戏运行面的完整审计.
# 参考项目与个人服务都从隔离目录启动, 整个过程保持无头运行.

[CmdletBinding()]
param(
    [string]$ReferenceRoot,
    [string]$ReferenceSourceRoot,
    [string]$ClientExecutable,
    [string]$CnCdnBundle,
    [string]$DecompiledRoot,
    [int]$RequestTimeoutMs = 120000
)

$ErrorActionPreference = 'Stop'
Set-StrictMode -Version Latest

# //// 运行 Node JSON 审计 [@x380kkm 2026-08-24] ////
function Invoke-NodeJsonAudit {
    param(
        [Parameter(Mandatory)][string]$ScriptPath,
        [Parameter(Mandatory)][string[]]$Arguments
    )

    $output = & node $ScriptPath @Arguments
    $exitCode = $LASTEXITCODE
    if ($exitCode -ne 0) {
        throw "reference audit failed: script=$ScriptPath, exit=$exitCode"
    }
    $output | ConvertFrom-Json -Depth 100
}
# //// /运行 Node JSON 审计 ////

# //// 运行双服务动态差分 [@x380kkm 2026-08-24] ////
function Invoke-DynamicReferenceAudit {
    param(
        [Parameter(Mandatory)][string]$ScriptPath,
        [Parameter(Mandatory)][string]$ReferenceProjectRoot,
        [Parameter(Mandatory)][int]$TimeoutMs
    )

    $output = & $ScriptPath -ReferenceRoot $ReferenceProjectRoot -RequestTimeoutMs $TimeoutMs
    $exitCode = $LASTEXITCODE
    if ($exitCode -ne 0 -and [string]::IsNullOrWhiteSpace(($output -join ''))) {
        throw "dynamic reference audit failed: exit=$exitCode"
    }
    $result = $output | ConvertFrom-Json -Depth 100
    $result | Add-Member -NotePropertyName runner_exit_code -NotePropertyValue $exitCode
    $result
}
# //// /运行双服务动态差分 ////

# //// 运行当前 iOS 客户端请求面审计 [@x380kkm 2026-08-24] ////
function Invoke-IosClientRouteAudit {
    param(
        [Parameter(Mandatory)][string]$ScriptPath,
        [Parameter(Mandatory)][string]$ExecutablePath,
        [Parameter(Mandatory)][string]$ClientSourceRoot,
        [Parameter(Mandatory)][string]$ReferenceProjectRoot
    )

    $output = & $ScriptPath `
        -ClientExecutable $ExecutablePath `
        -DecompiledRoot $ClientSourceRoot `
        -ReferenceRoot $ReferenceProjectRoot `
        -AllowMissing
    $exitCode = $LASTEXITCODE
    if ($exitCode -ne 0) {
        throw "iOS client route audit failed: exit=$exitCode"
    }
    $output | ConvertFrom-Json -Depth 100
}
# //// /运行当前 iOS 客户端请求面审计 ////

# //// 组合逐路由证据 [@x380kkm 2026-08-25] ////
function Get-RouteKey {
    param(
        [Parameter(Mandatory)][string]$Method,
        [Parameter(Mandatory)][string]$Path
    )

    '{0} {1}' -f $Method.ToUpperInvariant(), $Path
}

function Test-ContractMutation {
    param([object]$Contract)

    if ($null -eq $Contract -or $null -eq $Contract.state) {
        return $null
    }
    if (@($Contract.state.writeKeys).Count -gt 0) {
        return $true
    }
    $mutationPattern = '^(abort|add|apply|award|claim|collect|consume|continue|create|delete|deliver|disband|edit|exchange|finish|grant|inject|insert|learn|receive|recover|remove|reset|reward|save|sell|set|spend|start|update|upgrade)(_|[A-Z]|$)'
    foreach ($helper in @($Contract.state.helpers)) {
        if ([string]$helper -cmatch $mutationPattern) {
            return $true
        }
    }
    $false
}

function Get-OptionalPropertyValue {
    param(
        [AllowNull()][object]$Value,
        [Parameter(Mandatory)][string]$Name
    )

    if ($null -eq $Value) {
        return
    }
    if ($Value -is [System.Collections.IDictionary]) {
        if ($Value.Contains($Name) -and $null -ne $Value[$Name]) {
            $Value[$Name]
        }
        return
    }
    $property = $Value.PSObject.Properties[$Name]
    if ($null -eq $property) {
        return
    }
    if ($null -ne $property.Value) {
        $property.Value
    }
}

function Get-EmptyArrayPaths {
    param(
        [AllowNull()][object]$Value,
        [string]$Path = '$'
    )

    if ($null -eq $Value) {
        return
    }
    if ($Value -is [System.Collections.IList] -and $Value -isnot [string]) {
        if ($Value.Count -eq 0) {
            $Path
            return
        }
        for ($index = 0; $index -lt $Value.Count; $index++) {
            Get-EmptyArrayPaths -Value $Value[$index] -Path "$Path[$index]"
        }
        return
    }
    if ($Value -is [System.Collections.IDictionary]) {
        foreach ($name in $Value.Keys) {
            Get-EmptyArrayPaths -Value $Value[$name] -Path "$Path.$name"
        }
        return
    }
    if ($Value -is [System.Management.Automation.PSCustomObject]) {
        foreach ($property in $Value.PSObject.Properties) {
            Get-EmptyArrayPaths -Value $property.Value -Path "$Path.$($property.Name)"
        }
    }
}

function Test-IsActionArrayPath {
    param([Parameter(Mandatory)][string]$Path)

    $leaf = ($Path -split '\.')[-1]
    if ($leaf -eq 'mate_player_ids') {
        return $false
    }
    $leaf -match '(?:_list|_ids)$'
}

function Get-BusinessIdentifierEntries {
    param(
        [AllowNull()][object]$Value,
        [string]$Path = '$'
    )

    if ($null -eq $Value) {
        return
    }
    if ($Value -is [System.Collections.IList] -and $Value -isnot [string]) {
        for ($index = 0; $index -lt $Value.Count; $index++) {
            Get-BusinessIdentifierEntries -Value $Value[$index] -Path "$Path[$index]"
        }
        return
    }
    if ($Value -is [System.Collections.IDictionary]) {
        foreach ($name in $Value.Keys) {
            $childPath = "$Path.$name"
            if ([string]$name -match '(^id$|_id$|_ids$)') {
                [pscustomobject]@{ path = $childPath; value = $Value[$name] }
            }
            Get-BusinessIdentifierEntries -Value $Value[$name] -Path $childPath
        }
        return
    }
    if ($Value -is [System.Management.Automation.PSCustomObject]) {
        foreach ($property in $Value.PSObject.Properties) {
            $childPath = "$Path.$($property.Name)"
            if ($property.Name -match '(^id$|_id$|_ids$)') {
                [pscustomobject]@{ path = $childPath; value = $property.Value }
            }
            Get-BusinessIdentifierEntries -Value $property.Value -Path $childPath
        }
    }
}

function Get-BusinessIdentifierDifferencePaths {
    param(
        [AllowNull()][object]$ReferenceBody,
        [AllowNull()][object]$LocalBody
    )

    $referenceValues = @{}
    foreach ($entry in @(Get-BusinessIdentifierEntries -Value $ReferenceBody)) {
        $referenceValues[$entry.path] = $entry.value
    }
    $localValues = @{}
    foreach ($entry in @(Get-BusinessIdentifierEntries -Value $LocalBody)) {
        $localValues[$entry.path] = $entry.value
    }
    $paths = @($referenceValues.Keys) + @($localValues.Keys) | Sort-Object -Unique
    foreach ($path in $paths) {
        $referenceJson = if ($referenceValues.ContainsKey($path)) {
            $referenceValues[$path] | ConvertTo-Json -Compress -Depth 100
        } else {
            '<absent>'
        }
        $localJson = if ($localValues.ContainsKey($path)) {
            $localValues[$path] | ConvertTo-Json -Compress -Depth 100
        } else {
            '<absent>'
        }
        if ($referenceJson -ne $localJson) {
            $path
        }
    }
}

function Test-IsDomainPath {
    param([Parameter(Mandatory)][string]$Path)

    $Path -match '(?i)(character|equipment|gacha|item|mail|mission|shop|exchange|party|quest|reward|vmoney|mana|exp|stamina|token|point|stack|received|count|movie|seed|user_info|inventory|drop|folder|record|progress|currency|campaign|box)'
}

function Test-RouteMayMutate {
    param(
        [Parameter(Mandatory)][string]$Path,
        [AllowNull()][object]$ReferenceMutates,
        [AllowNull()][object]$LocalMutates
    )

    if ($ReferenceMutates -eq $true -or $LocalMutates -eq $true) {
        return $true
    }
    $Path -match '/(receive|sell|bulk|exec|exchange|edit|start|finish|buy|claim|update|create|delete|grant|reward|set|learn|upgrade|reset|continue|close|draw)$'
}

function Add-RouteGroupEntry {
    param(
        [Parameter(Mandatory)][hashtable]$Groups,
        [Parameter(Mandatory)][string]$Key,
        [Parameter(Mandatory)][object]$Value
    )

    if (-not $Groups.ContainsKey($Key)) {
        $Groups[$Key] = [System.Collections.Generic.List[object]]::new()
    }
    $Groups[$Key].Add($Value)
}

# //// 选择同初始状态的精确动态差异 [@x380kkm 2026-08-25] ////
function Get-StrongDynamicMismatchEvidence {
    param(
        [Parameter(Mandatory)][object[]]$DynamicCases,
        [Parameter(Mandatory)][object[]]$CaseQuality,
        [Parameter(Mandatory)][bool]$RouteMayMutate
    )

    $qualityById = @{}
    foreach ($quality in $CaseQuality) {
        $qualityById[[string](Get-OptionalPropertyValue -Value $quality -Name 'id')] = $quality
    }

    foreach ($dynamicCase in @($DynamicCases | Where-Object status -eq 'mismatched')) {
        $caseId = [string](Get-OptionalPropertyValue -Value $dynamicCase -Name 'id')
        $quality = $qualityById[$caseId]
        if ($null -eq $quality -or
            [string](Get-OptionalPropertyValue -Value $quality -Name 'strength') -ne 'strong') {
            continue
        }

        $probes = Get-OptionalPropertyValue -Value $dynamicCase -Name 'probes'
        $stateProbes = @(Get-OptionalPropertyValue -Value $probes -Name 'state')
        $beforeStatuses = @($stateProbes | ForEach-Object {
            $before = Get-OptionalPropertyValue -Value $_ -Name 'before'
            [string](Get-OptionalPropertyValue -Value $before -Name 'status')
        })
        $initialStateAligned = -not $RouteMayMutate -or
            ($stateProbes.Count -gt 0 -and
                @($beforeStatuses | Where-Object { $_ -ne 'matched' }).Count -eq 0)

        $primary = Get-OptionalPropertyValue -Value $dynamicCase -Name 'primary'
        $responseMismatched = [string](Get-OptionalPropertyValue `
            -Value $primary `
            -Name 'status') -eq 'mismatched'
        $transitionStatuses = @($stateProbes | ForEach-Object {
            $transition = Get-OptionalPropertyValue -Value $_ -Name 'transition'
            [string](Get-OptionalPropertyValue -Value $transition -Name 'status')
        })
        $stateMismatched = $RouteMayMutate -and
            $transitionStatuses -contains 'mismatched'

        if ($initialStateAligned -and ($responseMismatched -or $stateMismatched)) {
            [pscustomobject][ordered]@{
                id = $caseId
                response = $responseMismatched
                state = $stateMismatched
                state_probes = $stateProbes.Count
                initial_state = if ($RouteMayMutate) { 'matched' } else { 'not-applicable' }
            }
        }
    }
}
# //// /选择同初始状态的精确动态差异 ////

function New-RouteEvidenceMatrix {
    param(
        [Parameter(Mandatory)][object]$RouteAudit,
        [Parameter(Mandatory)][object]$TypeScriptAudit,
        [Parameter(Mandatory)][object]$DynamicReport,
        [Parameter(Mandatory)][object]$Corpus,
        [Parameter(Mandatory)][object]$IosAudit
    )

    $compiledContracts = @{}
    foreach ($route in @($RouteAudit.contracts.routes)) {
        $compiledContracts[(Get-RouteKey -Method $route.method -Path $route.path)] = $route
    }
    $sourceContracts = @{}
    foreach ($route in @($TypeScriptAudit.routes)) {
        $sourceContracts[(Get-RouteKey -Method $route.method -Path $route.path)] = $route
    }
    $dynamicGroups = @{}
    foreach ($route in @($DynamicReport.routes)) {
        Add-RouteGroupEntry -Groups $dynamicGroups `
            -Key (Get-RouteKey -Method $route.method -Path $route.path) `
            -Value $route
    }
    $corpusGroups = @{}
    foreach ($case in @($Corpus.cases)) {
        Add-RouteGroupEntry -Groups $corpusGroups `
            -Key (Get-RouteKey -Method $case.method -Path $case.path) `
            -Value $case
    }
    $clientPaths = @{}
    foreach ($route in @($IosAudit.current_routes)) {
        $clientPaths[[string]$route.path] = $route
    }

    foreach ($referenceRoute in @($RouteAudit.covered.routes)) {
        $key = Get-RouteKey -Method $referenceRoute.method -Path $referenceRoute.path
        $compiled = $compiledContracts[$key]
        $source = $sourceContracts[$key]
        $dynamicCases = @($dynamicGroups[$key])
        $caseDefinitions = @($corpusGroups[$key])
        $compiledStatus = Get-OptionalPropertyValue -Value $compiled -Name 'status'
        $sourceStatus = Get-OptionalPropertyValue -Value $source -Name 'status'
        $sourceSuccess = Get-OptionalPropertyValue -Value $source -Name 'success'
        $sourceErrors = Get-OptionalPropertyValue -Value $source -Name 'errors'
        $sourceState = Get-OptionalPropertyValue -Value $source -Name 'state'
        $compiledReferenceContract = Get-OptionalPropertyValue -Value $referenceRoute -Name 'contract'
        $compiledLocalContract = Get-OptionalPropertyValue -Value $referenceRoute -Name 'rustContract'
        $sourceReferenceContract = Get-OptionalPropertyValue -Value $source -Name 'referenceContract'
        $sourceLocalContract = Get-OptionalPropertyValue -Value $source -Name 'rustContract'
        $referenceMutates = (Test-ContractMutation -Contract $compiledReferenceContract) -eq $true -or
            (Test-ContractMutation -Contract $sourceReferenceContract) -eq $true
        $localMutates = (Test-ContractMutation -Contract $compiledLocalContract) -eq $true -or
            (Test-ContractMutation -Contract $sourceLocalContract) -eq $true
        $routeMayMutate = Test-RouteMayMutate `
            -Path ([string]$referenceRoute.path) `
            -ReferenceMutates $referenceMutates `
            -LocalMutates $localMutates
        $sourceStateStatus = Get-OptionalPropertyValue -Value $sourceState -Name 'status'
        $bothMutateUnverified = $referenceMutates -and $localMutates -and
            $sourceStateStatus -ne 'matched'

        $caseQuality = @(
            foreach ($caseDefinition in $caseDefinitions) {
                $caseId = [string](Get-OptionalPropertyValue -Value $caseDefinition -Name 'id')
                $referenceBody = Get-OptionalPropertyValue -Value $caseDefinition -Name 'body'
                $localDefinition = Get-OptionalPropertyValue -Value $caseDefinition -Name 'rust'
                $localBodyOverride = Get-OptionalPropertyValue -Value $localDefinition -Name 'body'
                $localBody = if ($null -eq $localBodyOverride) {
                    $referenceBody
                } else {
                    $localBodyOverride
                }
                $referenceBodyJson = $referenceBody | ConvertTo-Json -Compress -Depth 100
                $localBodyJson = $localBody | ConvertTo-Json -Compress -Depth 100
                $bodyDiffers = $referenceBodyJson -ne $localBodyJson
                $emptyArrayPaths = @(Get-EmptyArrayPaths -Value $referenceBody | Sort-Object -Unique)
                $actionEmptyArrayPaths = @(if ($routeMayMutate) {
                    $emptyArrayPaths | Where-Object { Test-IsActionArrayPath -Path ([string]$_) }
                })
                $identifierDifferences = @(if ($bodyDiffers) {
                    Get-BusinessIdentifierDifferencePaths `
                        -ReferenceBody $referenceBody `
                        -LocalBody $localBody | Sort-Object -Unique
                })

                $comparison = Get-OptionalPropertyValue -Value $caseDefinition -Name 'comparison'
                $normalization = Get-OptionalPropertyValue -Value $caseDefinition -Name 'normalize'
                $responseIgnoredPaths = @(
                    @(Get-OptionalPropertyValue -Value $comparison -Name 'ignorePaths') +
                        @(Get-OptionalPropertyValue -Value $normalization -Name 'ignorePaths') |
                        Where-Object { -not [string]::IsNullOrWhiteSpace([string]$_) } |
                        Sort-Object -Unique
                )
                $responseNormalizedPaths = @(
                    @(Get-OptionalPropertyValue -Value $normalization -Name 'paths') +
                        @(Get-OptionalPropertyValue -Value $normalization -Name 'zeroBaselinePaths') +
                        @(Get-OptionalPropertyValue -Value $comparison -Name 'mapValues') |
                        Where-Object { -not [string]::IsNullOrWhiteSpace([string]$_) } |
                        Sort-Object -Unique
                )
                $responseValuePaths = @(
                    Get-OptionalPropertyValue -Value $comparison -Name 'valuePaths' |
                        Where-Object { -not [string]::IsNullOrWhiteSpace([string]$_) } |
                        Sort-Object -Unique
                )

                $probes = Get-OptionalPropertyValue -Value $caseDefinition -Name 'probes'
                $caseStateProbes = @(Get-OptionalPropertyValue -Value $probes -Name 'state')
                $stateIgnoredPaths = @(
                    @(Get-OptionalPropertyValue -Value $caseDefinition -Name 'stateIgnorePaths') +
                        @($caseStateProbes | ForEach-Object {
                            @(Get-OptionalPropertyValue -Value $_ -Name 'stateIgnorePaths')
                        }) |
                        Where-Object { -not [string]::IsNullOrWhiteSpace([string]$_) } |
                        Sort-Object -Unique
                )
                $caseStateComparison = Get-OptionalPropertyValue `
                    -Value $caseDefinition `
                    -Name 'stateComparison'
                $stateComparisonModes = @(
                    @($caseStateComparison) + @($caseStateProbes | ForEach-Object {
                        Get-OptionalPropertyValue -Value $_ -Name 'stateComparison'
                    }) |
                        Where-Object { -not [string]::IsNullOrWhiteSpace([string]$_) } |
                        Sort-Object -Unique
                )
                if ($stateComparisonModes.Count -eq 0 -and $caseStateProbes.Count -gt 0) {
                    $stateComparisonModes = @('exact')
                }
                $stateProjection = Get-OptionalPropertyValue -Value $caseDefinition -Name 'stateProjection'
                $stateProjectionPaths = @(
                    @(Get-OptionalPropertyValue -Value $stateProjection -Name 'valuePaths') +
                        @($caseStateProbes | ForEach-Object {
                            $probeProjection = Get-OptionalPropertyValue -Value $_ -Name 'stateProjection'
                            @(Get-OptionalPropertyValue -Value $probeProjection -Name 'valuePaths')
                        }) |
                        Where-Object { -not [string]::IsNullOrWhiteSpace([string]$_) } |
                        Sort-Object -Unique
                )
                $domainHiddenPaths = @(
                    @($responseIgnoredPaths) + @($responseNormalizedPaths) + @($stateIgnoredPaths) |
                        Where-Object { Test-IsDomainPath -Path ([string]$_) } |
                        Sort-Object -Unique
                )
                $targetOverride = Get-OptionalPropertyValue -Value $caseDefinition -Name 'targetOverride'
                $targetOverrideKind = [string](Get-OptionalPropertyValue `
                    -Value $targetOverride `
                    -Name 'kind')
                $targetOverrideReason = [string](Get-OptionalPropertyValue `
                    -Value $targetOverride `
                    -Name 'reason')
                $targetOverrideEvidence = @(
                    Get-OptionalPropertyValue -Value $targetOverride -Name 'evidence' |
                        Where-Object { -not [string]::IsNullOrWhiteSpace([string]$_) }
                )
                $targetOverrideValid = $targetOverrideKind -eq 'reference-defect' -and
                    -not [string]::IsNullOrWhiteSpace($targetOverrideReason) -and
                    $targetOverrideEvidence.Count -gt 0
                $targetMismatch = Get-OptionalPropertyValue -Value $caseDefinition -Name 'targetMismatch'
                $targetMismatchKind = [string](Get-OptionalPropertyValue `
                    -Value $targetMismatch `
                    -Name 'kind')
                $targetMismatchReason = [string](Get-OptionalPropertyValue `
                    -Value $targetMismatch `
                    -Name 'reason')
                $targetMismatchEvidence = @(
                    Get-OptionalPropertyValue -Value $targetMismatch -Name 'evidence' |
                        Where-Object { -not [string]::IsNullOrWhiteSpace([string]$_) }
                )
                $targetMismatchValid = $targetMismatchKind -eq 'local-defect' -and
                    -not [string]::IsNullOrWhiteSpace($targetMismatchReason) -and
                    $targetMismatchEvidence.Count -gt 0

                $weakReasons = [System.Collections.Generic.List[string]]::new()
                if ($actionEmptyArrayPaths.Count -gt 0) {
                    $weakReasons.Add('action-empty-array-input')
                }
                if ($bodyDiffers) {
                    $weakReasons.Add('side-specific-request-body')
                }
                if ($identifierDifferences.Count -gt 0) {
                    $weakReasons.Add('business-identifier-mismatch')
                }
                if ($domainHiddenPaths.Count -gt 0) {
                    $weakReasons.Add('ignored-or-normalized-domain-path')
                }
                if ($stateComparisonModes -contains 'change-presence') {
                    $weakReasons.Add('change-presence-state-comparison')
                }
                if ($routeMayMutate -and $caseStateProbes.Count -eq 0) {
                    $weakReasons.Add('mutation-without-state-probe')
                }
                if ($responseValuePaths.Count -gt 0) {
                    $weakReasons.Add('partial-response-projection')
                }
                if ($stateProjectionPaths.Count -gt 0) {
                    $weakReasons.Add('partial-state-projection')
                }

                $dynamicStatuses = @(
                    $dynamicCases |
                        Where-Object {
                            [string](Get-OptionalPropertyValue -Value $_ -Name 'id') -eq $caseId
                        } |
                        ForEach-Object { [string]$_.status } |
                        Sort-Object -Unique
                )
                $pseudoMatched = $dynamicStatuses -contains 'matched' -and $weakReasons.Count -gt 0
                [pscustomobject][ordered]@{
                    id = $caseId
                    dynamic_statuses = $dynamicStatuses
                    strength = if ($weakReasons.Count -eq 0) { 'strong' } else { 'weak' }
                    weak_reasons = @($weakReasons | Sort-Object -Unique)
                    empty_array_paths = $emptyArrayPaths
                    action_empty_array_paths = $actionEmptyArrayPaths
                    side_specific_body = $bodyDiffers
                    business_identifier_differences = $identifierDifferences
                    response_ignored_paths = $responseIgnoredPaths
                    response_normalized_paths = $responseNormalizedPaths
                    response_value_paths = $responseValuePaths
                    state_probes = $caseStateProbes.Count
                    state_comparison_modes = $stateComparisonModes
                    state_ignored_paths = $stateIgnoredPaths
                    state_projection_paths = $stateProjectionPaths
                    ignored_domain_paths = $domainHiddenPaths
                    target_override = if ($null -eq $targetOverride) {
                        $null
                    } else {
                        [ordered]@{
                            valid = $targetOverrideValid
                            kind = $targetOverrideKind
                            reason = $targetOverrideReason
                            evidence = $targetOverrideEvidence
                        }
                    }
                    target_mismatch = if ($null -eq $targetMismatch) {
                        $null
                    } else {
                        [ordered]@{
                            valid = $targetMismatchValid
                            kind = $targetMismatchKind
                            reason = $targetMismatchReason
                            evidence = $targetMismatchEvidence
                        }
                    }
                    pseudo_matched = $pseudoMatched
                }
            }
        )

        $stateProbeCount = @($caseQuality | ForEach-Object { $_.state_probes } |
            Measure-Object -Sum).Sum
        if ($null -eq $stateProbeCount) {
            $stateProbeCount = 0
        }
        $responseIgnoredPaths = @(
            $caseQuality | ForEach-Object { $_.response_ignored_paths } | Sort-Object -Unique
        )
        $stateIgnoredPaths = @(
            $caseQuality | ForEach-Object { $_.state_ignored_paths } | Sort-Object -Unique
        )
        $ignoredPaths = @(@($responseIgnoredPaths) + @($stateIgnoredPaths) | Sort-Object -Unique)
        $normalizedPaths = @(
            $caseQuality | ForEach-Object { $_.response_normalized_paths } | Sort-Object -Unique
        )
        $responseDomainHiddenPaths = @(
            @($responseIgnoredPaths) + @($normalizedPaths) |
                Where-Object { Test-IsDomainPath -Path ([string]$_) } |
                Sort-Object -Unique
        )
        $stateDomainHiddenPaths = @(
            $stateIgnoredPaths |
                Where-Object { Test-IsDomainPath -Path ([string]$_) } |
                Sort-Object -Unique
        )
        $domainHiddenPaths = @(
            @($responseDomainHiddenPaths) + @($stateDomainHiddenPaths) | Sort-Object -Unique
        )
        $usesChangePresence = @($caseQuality | Where-Object {
            $_.state_comparison_modes -contains 'change-presence'
        }).Count -gt 0
        $hasSideSpecificBody = @($caseQuality | Where-Object side_specific_body).Count -gt 0
        $identifierDifferencePaths = @(
            $caseQuality | ForEach-Object { $_.business_identifier_differences } |
                Sort-Object -Unique
        )
        $actionEmptyArrayPaths = @(
            $caseQuality | ForEach-Object { $_.action_empty_array_paths } | Sort-Object -Unique
        )
        $usesEmptyArray = $actionEmptyArrayPaths.Count -gt 0
        $usesIgnoredIdentifiers = $domainHiddenPaths.Count -gt 0

        $dynamicStatus = if ($dynamicCases.Count -eq 0) {
            'absent'
        } elseif (@($dynamicCases | Where-Object status -eq 'mismatched').Count -gt 0) {
            'mismatched'
        } elseif (@($dynamicCases | Where-Object status -eq 'unresolved').Count -gt 0) {
            'unresolved'
        } elseif (@($dynamicCases | Where-Object status -eq 'local-extension').Count -gt 0) {
            'local-extension'
        } else {
            'matched'
        }
        $pseudoMatchedCases = @($caseQuality | Where-Object pseudo_matched)
        $pseudoMatched = $pseudoMatchedCases.Count -gt 0 -or
            ($dynamicStatus -eq 'matched' -and $bothMutateUnverified)
        $pseudoMatchedReasons = @(
            @($pseudoMatchedCases | ForEach-Object { $_.weak_reasons }) +
                @(if ($dynamicStatus -eq 'matched' -and $bothMutateUnverified) {
                    'mutation-equivalence-unverified'
                }) |
                Where-Object { -not [string]::IsNullOrWhiteSpace([string]$_) } |
                Sort-Object -Unique
        )
        $targetOverrideCases = @($caseQuality | Where-Object {
            $override = Get-OptionalPropertyValue -Value $_ -Name 'target_override'
            (Get-OptionalPropertyValue -Value $override -Name 'valid') -eq $true
        })
        $invalidTargetOverrideCases = @($caseQuality | Where-Object {
            $override = Get-OptionalPropertyValue -Value $_ -Name 'target_override'
            $null -ne $override -and
                (Get-OptionalPropertyValue -Value $override -Name 'valid') -ne $true
        })
        $targetOverrideIds = @($targetOverrideCases | ForEach-Object {
            Get-OptionalPropertyValue -Value $_ -Name 'id'
        })
        $dynamicMismatchCases = @($dynamicCases | Where-Object status -eq 'mismatched')
        $targetOverrideApplied = $dynamicMismatchCases.Count -gt 0 -and
            @($dynamicMismatchCases | Where-Object {
                $targetOverrideIds -notcontains [string](Get-OptionalPropertyValue -Value $_ -Name 'id')
            }).Count -eq 0
        $targetMismatchCases = @($caseQuality | Where-Object {
            $targetMismatch = Get-OptionalPropertyValue -Value $_ -Name 'target_mismatch'
            (Get-OptionalPropertyValue -Value $targetMismatch -Name 'valid') -eq $true
        })
        $invalidTargetMismatchCases = @($caseQuality | Where-Object {
            $targetMismatch = Get-OptionalPropertyValue -Value $_ -Name 'target_mismatch'
            $null -ne $targetMismatch -and
                (Get-OptionalPropertyValue -Value $targetMismatch -Name 'valid') -ne $true
        })

        $compiledDifferences = @(
            Get-OptionalPropertyValue -Value $compiled -Name 'differences' |
                Where-Object { $null -ne $_ }
        )
        $reliableCompiledMismatch = $compiledStatus -eq 'mismatched' -and
            $null -ne $compiledReferenceContract -and
            $null -ne $compiledLocalContract -and
            $compiledDifferences.Count -gt 0
        $strongDynamicMismatchEvidence = @(Get-StrongDynamicMismatchEvidence `
            -DynamicCases $dynamicCases `
            -CaseQuality $caseQuality `
            -RouteMayMutate $routeMayMutate)

        $rawMismatchSources = @(
            if ($compiledStatus -eq 'mismatched') { 'compiled' }
            if ($sourceStatus -eq 'mismatched') { 'source' }
            if ($dynamicMismatchCases.Count -gt 0) { 'dynamic' }
        )
        $hasRawMismatch = $rawMismatchSources.Count -gt 0
        $targetMismatchApplied = $hasRawMismatch -and $targetMismatchCases.Count -gt 0
        $mismatchEvidence = @(
            if ($targetOverrideApplied) { 'target-override' }
            if ($targetMismatchApplied) { 'target-capture' }
            if ($reliableCompiledMismatch) { 'compiled-contract' }
            if ($strongDynamicMismatchEvidence.Count -gt 0) { 'strong-dynamic' }
        )

        $sourceSuccessStatus = [string](Get-OptionalPropertyValue `
            -Value $sourceSuccess `
            -Name 'status')
        $sourceErrorsStatus = [string](Get-OptionalPropertyValue `
            -Value $sourceErrors `
            -Name 'status')
        $dynamicResponseMismatchCases = @($dynamicMismatchCases | Where-Object {
            $primary = Get-OptionalPropertyValue -Value $_ -Name 'primary'
            [string](Get-OptionalPropertyValue -Value $primary -Name 'status') -eq 'mismatched'
        })
        $dynamicStateMismatchCases = @($dynamicMismatchCases | Where-Object {
            $probes = Get-OptionalPropertyValue -Value $_ -Name 'probes'
            $stateProbes = @(Get-OptionalPropertyValue -Value $probes -Name 'state')
            @($stateProbes | Where-Object {
                $transition = Get-OptionalPropertyValue -Value $_ -Name 'transition'
                [string](Get-OptionalPropertyValue -Value $transition -Name 'status') -eq 'mismatched'
            }).Count -gt 0
        })
        $rawResponseMismatch = $compiledStatus -eq 'mismatched' -or
            $sourceSuccessStatus -eq 'mismatched' -or
            $sourceErrorsStatus -eq 'mismatched' -or
            $dynamicResponseMismatchCases.Count -gt 0
        $rawStateMismatch = $sourceStateStatus -eq 'mismatched' -or
            $sourceStateStatus -eq 'local-extension' -or
            $dynamicStateMismatchCases.Count -gt 0

        $flags = [System.Collections.Generic.List[string]]::new()
        if ($null -eq $compiled) {
            $flags.Add('compiled-contract-absent')
        } elseif ($compiledStatus -ne 'matched') {
            $flags.Add("compiled-contract-$compiledStatus")
        }
        if ($null -eq $source) {
            $flags.Add('source-contract-absent')
        } elseif ($sourceStatus -ne 'matched') {
            $flags.Add("source-contract-$sourceStatus")
        }
        if ($dynamicCases.Count -eq 0) {
            $flags.Add('dynamic-case-absent')
        } else {
            foreach ($status in @($dynamicCases.status | Sort-Object -Unique)) {
                if ($status -ne 'matched' -and $status -ne 'local-extension') {
                    $flags.Add("dynamic-$status")
                }
            }
        }
        if ($referenceMutates -eq $true -and $stateProbeCount -eq 0) {
            $flags.Add('mutation-without-state-probe')
        }
        if ($bothMutateUnverified) {
            $flags.Add('mutation-equivalence-unverified')
        }
        if ($usesChangePresence) {
            $flags.Add('change-presence-state-comparison')
        }
        if ($usesEmptyArray) {
            $flags.Add('empty-array-input')
        }
        if ($hasSideSpecificBody) {
            $flags.Add('side-specific-request-body')
        }
        if ($identifierDifferencePaths.Count -gt 0) {
            $flags.Add('business-identifier-mismatch')
        }
        if ($ignoredPaths.Count -gt 0) {
            $flags.Add('ignored-response-or-state-paths')
        }
        if ($usesIgnoredIdentifiers) {
            $flags.Add('ignored-domain-identifiers')
        }
        if (@($caseQuality | Where-Object { $_.response_value_paths.Count -gt 0 }).Count -gt 0) {
            $flags.Add('partial-response-projection')
        }
        if (@($caseQuality | Where-Object { $_.state_projection_paths.Count -gt 0 }).Count -gt 0) {
            $flags.Add('partial-state-projection')
        }
        if ($pseudoMatched) {
            $flags.Add('pseudo-matched-dynamic-case')
        }
        if ($targetOverrideApplied) {
            $flags.Add('target-reference-defect')
        }
        if ($targetMismatchApplied) {
            $flags.Add('target-capture-mismatch')
        }
        if ($reliableCompiledMismatch) {
            $flags.Add('reliable-compiled-mismatch')
        }
        if ($strongDynamicMismatchEvidence.Count -gt 0) {
            $flags.Add('strong-dynamic-mismatch')
        }
        if ($invalidTargetOverrideCases.Count -gt 0) {
            $flags.Add('invalid-target-override')
        }
        if ($invalidTargetMismatchCases.Count -gt 0) {
            $flags.Add('invalid-target-mismatch')
        }

        $verdict = if ($targetOverrideApplied) {
            'target-override'
        } elseif ($mismatchEvidence.Count -gt 0) {
            'mismatched'
        } elseif ($flags.Count -gt 0) {
            'unresolved'
        } else {
            'verified'
        }
        if ($hasRawMismatch -and $verdict -eq 'unresolved') {
            $flags.Add('raw-mismatch-downgraded')
        }
        $mismatchDowngradeReasons = if ($hasRawMismatch -and $verdict -eq 'unresolved') {
            @($flags | Sort-Object -Unique)
        } else {
            @()
        }
        $inputWeakCaseCount = @($caseQuality | Where-Object strength -eq 'weak').Count
        $inputStrength = if ($caseDefinitions.Count -eq 0) {
            'absent'
        } elseif ($inputWeakCaseCount -eq 0) {
            'strong'
        } elseif ($inputWeakCaseCount -eq $caseDefinitions.Count) {
            'weak'
        } else {
            'mixed'
        }
        $responseComparisonStatus = if ($targetOverrideApplied) {
            'target-override'
        } elseif ($rawResponseMismatch -and $verdict -eq 'mismatched') {
            'mismatched'
        } elseif ($rawResponseMismatch) {
            'unresolved'
        } elseif ($responseIgnoredPaths.Count -gt 0 -or $normalizedPaths.Count -gt 0 -or
            @($caseQuality | Where-Object { $_.response_value_paths.Count -gt 0 }).Count -gt 0) {
            'partial'
        } elseif ($compiledStatus -eq 'unresolved' -or $sourceStatus -eq 'unresolved' -or
            $dynamicStatus -eq 'unresolved' -or $dynamicStatus -eq 'absent') {
            'unresolved'
        } else {
            'exact'
        }
        $stateComparisonStatus = if ($targetOverrideApplied) {
            'target-override'
        } elseif (-not $routeMayMutate) {
            'not-applicable'
        } elseif ($rawStateMismatch -and $verdict -eq 'mismatched') {
            'mismatched'
        } elseif ($rawStateMismatch) {
            'unresolved'
        } elseif ($stateProbeCount -eq 0 -or $usesChangePresence -or
            $bothMutateUnverified -or $stateDomainHiddenPaths.Count -gt 0 -or
            @($caseQuality | Where-Object { $_.state_projection_paths.Count -gt 0 }).Count -gt 0) {
            'partial'
        } elseif ($dynamicStatus -eq 'matched') {
            'exact'
        } else {
            'unresolved'
        }

        [pscustomobject][ordered]@{
            method = $referenceRoute.method
            path = $referenceRoute.path
            reference_handler = $referenceRoute.source
            local_handlers = @($referenceRoute.rustSources)
            reference_source = $referenceRoute.source
            rust_sources = @($referenceRoute.rustSources)
            current_ios = $clientPaths.ContainsKey([string]$referenceRoute.path)
            reference_mutates = $referenceMutates
            local_mutates = $localMutates
            input = [ordered]@{
                strength = $inputStrength
                cases = $caseDefinitions.Count
                weak_cases = $inputWeakCaseCount
                action_empty_array_paths = $actionEmptyArrayPaths
                business_identifier_differences = $identifierDifferencePaths
                side_specific_request_body = $hasSideSpecificBody
                case_evidence = $caseQuality
            }
            response_comparison = [ordered]@{
                status = $responseComparisonStatus
                compiled = if ($null -eq $compiled) { 'absent' } else { $compiledStatus }
                source = if ($null -eq $source) { 'absent' } else { $sourceStatus }
                dynamic = $dynamicStatus
                ignored_paths = $responseIgnoredPaths
                normalized_paths = $normalizedPaths
                hidden_domain_paths = $responseDomainHiddenPaths
            }
            state_comparison = [ordered]@{
                status = $stateComparisonStatus
                reference_mutates = $referenceMutates
                local_mutates = $localMutates
                probes = $stateProbeCount
                modes = @($caseQuality | ForEach-Object { $_.state_comparison_modes } |
                    Sort-Object -Unique)
                ignored_paths = $stateIgnoredPaths
                hidden_domain_paths = $stateDomainHiddenPaths
                mutation_equivalence_unverified = $bothMutateUnverified
            }
            compiled_contract = if ($null -eq $compiled) { 'absent' } else { $compiledStatus }
            compiled_differences = $compiledDifferences
            source_contract = if ($null -eq $source) { 'absent' } else { $sourceStatus }
            source_success_differences = @(
                Get-OptionalPropertyValue -Value $sourceSuccess -Name 'differences' |
                    Where-Object { $null -ne $_ }
            )
            source_error_missing = @(
                Get-OptionalPropertyValue -Value $sourceErrors -Name 'missingStatuses' |
                    Where-Object { $null -ne $_ }
            )
            source_error_extra = @(
                Get-OptionalPropertyValue -Value $sourceErrors -Name 'extraStatuses' |
                    Where-Object { $null -ne $_ }
            )
            source_state = if ($null -eq $source) {
                'absent'
            } else {
                Get-OptionalPropertyValue -Value $sourceState -Name 'status'
            }
            dynamic = $dynamicStatus
            dynamic_cases = $dynamicCases.Count
            state_probes = $stateProbeCount
            ignored_paths = @($ignoredPaths | Sort-Object -Unique)
            evidence_flags = @($flags | Sort-Object -Unique)
            uncovered_reasons = @($flags | Sort-Object -Unique)
            raw_mismatch = [ordered]@{
                present = $hasRawMismatch
                sources = $rawMismatchSources
                response = $rawResponseMismatch
                state = $rawStateMismatch
                downgraded = $hasRawMismatch -and $verdict -eq 'unresolved'
                downgrade_reasons = @($mismatchDowngradeReasons)
            }
            mismatch_evidence = [ordered]@{
                accepted = $mismatchEvidence
                reliable_compiled = $reliableCompiledMismatch
                strong_dynamic = $strongDynamicMismatchEvidence
                target_capture = @($targetMismatchCases | ForEach-Object {
                    Get-OptionalPropertyValue -Value $_ -Name 'target_mismatch'
                })
            }
            target_overrides = @($targetOverrideCases | ForEach-Object {
                Get-OptionalPropertyValue -Value $_ -Name 'target_override'
            })
            pseudo_matched = $pseudoMatched
            pseudo_matched_cases = @($pseudoMatchedCases | ForEach-Object {
                Get-OptionalPropertyValue -Value $_ -Name 'id'
            })
            pseudo_matched_reasons = $pseudoMatchedReasons
            verdict = $verdict
            conclusion = $verdict
        }
    }
}
# //// /组合逐路由证据 ////

# //// 生成参考证据门禁 [@x380kkm 2026-08-25] ////
function New-ReferenceEvidenceGate {
    param(
        [Parameter(Mandatory)][object[]]$ReferenceRoutes,
        [Parameter(Mandatory)][object[]]$RouteMatrix
    )

    $referenceRouteKeys = @($ReferenceRoutes | ForEach-Object {
        Get-RouteKey -Method ([string]$_.method) -Path ([string]$_.path)
    } | Sort-Object -Unique)
    $matrixRouteEntries = @($RouteMatrix | ForEach-Object {
        [pscustomobject][ordered]@{
            key = Get-RouteKey -Method ([string]$_.method) -Path ([string]$_.path)
            route = $_
        }
    })
    $matrixRouteKeys = @($matrixRouteEntries.key | Sort-Object -Unique)
    $missingRouteKeys = @($referenceRouteKeys | Where-Object {
        $matrixRouteKeys -notcontains $_
    })
    $unexpectedRouteKeys = @($matrixRouteKeys | Where-Object {
        $referenceRouteKeys -notcontains $_
    })
    $duplicateRoutes = @($matrixRouteEntries | Group-Object key | Where-Object Count -gt 1 |
        ForEach-Object {
            [ordered]@{
                route_key = $_.Name
                count = $_.Count
            }
        })
    $pseudoMatchedRoutes = @($RouteMatrix | Where-Object pseudo_matched | ForEach-Object {
        [ordered]@{
            method = $_.method
            path = $_.path
            cases = $_.pseudo_matched_cases
            reasons = $_.pseudo_matched_reasons
        }
    })
    $mismatchedRoutes = @($RouteMatrix | Where-Object verdict -eq 'mismatched' | ForEach-Object {
        [ordered]@{
            method = $_.method
            path = $_.path
            evidence = $_.mismatch_evidence.accepted
        }
    })
    $invalidTargetOverrideRoutes = @($RouteMatrix | Where-Object {
        $_.evidence_flags -contains 'invalid-target-override'
    } | ForEach-Object {
        $invalidCases = @($_.input.case_evidence | Where-Object {
            $null -ne $_.target_override -and $_.target_override.valid -ne $true
        } | ForEach-Object {
            [ordered]@{
                id = $_.id
                kind = $_.target_override.kind
                reason = $_.target_override.reason
                evidence = $_.target_override.evidence
            }
        })
        [ordered]@{
            method = $_.method
            path = $_.path
            cases = $invalidCases
        }
    })
    $unresolvedRoutes = @($RouteMatrix | Where-Object verdict -eq 'unresolved' | ForEach-Object {
        [ordered]@{
            method = $_.method
            path = $_.path
            reasons = $_.evidence_flags
        }
    })
    $targetOverrideRoutes = @($RouteMatrix | Where-Object verdict -eq 'target-override' |
        ForEach-Object {
            [ordered]@{
                method = $_.method
                path = $_.path
                evidence = $_.target_overrides
            }
        })

    $coverageComplete = $missingRouteKeys.Count -eq 0 -and $unexpectedRouteKeys.Count -eq 0
    $implementationConsistent = $coverageComplete -and
        $duplicateRoutes.Count -eq 0 -and
        $mismatchedRoutes.Count -eq 0 -and
        $invalidTargetOverrideRoutes.Count -eq 0
    $evidenceConclusive = $pseudoMatchedRoutes.Count -eq 0 -and $unresolvedRoutes.Count -eq 0
    $releaseReady = $implementationConsistent -and $evidenceConclusive

    $checks = [ordered]@{
        coverage = [ordered]@{
            passed = $coverageComplete
            count = $missingRouteKeys.Count + $unexpectedRouteKeys.Count
            reference_routes = $referenceRouteKeys.Count
            matrix_routes = $matrixRouteKeys.Count
            missing_route_keys = $missingRouteKeys
            unexpected_route_keys = $unexpectedRouteKeys
        }
        duplicate_routes = [ordered]@{
            passed = $duplicateRoutes.Count -eq 0
            count = $duplicateRoutes.Count
            routes = $duplicateRoutes
        }
        mismatched = [ordered]@{
            passed = $mismatchedRoutes.Count -eq 0
            count = $mismatchedRoutes.Count
            routes = $mismatchedRoutes
        }
        invalid_target_override = [ordered]@{
            passed = $invalidTargetOverrideRoutes.Count -eq 0
            count = $invalidTargetOverrideRoutes.Count
            routes = $invalidTargetOverrideRoutes
        }
        pseudo_matched = [ordered]@{
            passed = $pseudoMatchedRoutes.Count -eq 0
            count = $pseudoMatchedRoutes.Count
            routes = $pseudoMatchedRoutes
        }
        unresolved = [ordered]@{
            passed = $unresolvedRoutes.Count -eq 0
            count = $unresolvedRoutes.Count
            routes = $unresolvedRoutes
        }
    }
    $blockers = @($checks.GetEnumerator() | Where-Object { -not $_.Value.passed } |
        ForEach-Object {
            [ordered]@{
                check = $_.Key
                count = $_.Value.count
                details = $_.Value
            }
        })

    [pscustomobject][ordered]@{
        kind = 'reference-consistency-and-evidence'
        definition = 'implementation_consistent 检查实现差异. evidence_conclusive 检查证据完整性.'
        passed = $releaseReady
        release_ready = $releaseReady
        implementation_consistent = $implementationConsistent
        evidence_conclusive = $evidenceConclusive
        checks = $checks
        blockers = $blockers
        target_overrides = [ordered]@{
            count = $targetOverrideRoutes.Count
            routes = $targetOverrideRoutes
        }
    }
}
# //// /生成参考证据门禁 ////

# //// 汇总完整参考核对 [@x380kkm 2026-08-24] ////
$repositoryRoot = (Resolve-Path -LiteralPath (Join-Path $PSScriptRoot '..\..')).Path
$workspaceRoot = Split-Path -Parent $repositoryRoot
if ([string]::IsNullOrWhiteSpace($ReferenceRoot)) {
    $ReferenceRoot = Join-Path $workspaceRoot 'startpoint-cn-launcher'
}
if ([string]::IsNullOrWhiteSpace($ReferenceSourceRoot)) {
    $ReferenceSourceRoot = Join-Path $workspaceRoot 'startpoint-cn'
}
if ([string]::IsNullOrWhiteSpace($ClientExecutable)) {
    $ClientExecutable = Join-Path $workspaceRoot `
        'starpoint\artifacts\ios-device-staging\jp-art-final\Payload\worldflipper.app\worldflipper'
}
if ([string]::IsNullOrWhiteSpace($CnCdnBundle)) {
    $CnCdnBundle = Join-Path $workspaceRoot `
        'starpoint\artifacts\ios-device-staging\jp-art-final\Payload\worldflipper.app\StarpointCNCDN'
}
if ([string]::IsNullOrWhiteSpace($DecompiledRoot)) {
    $DecompiledRoot = Join-Path $workspaceRoot 'wf-2.1.125-cn-decompiled'
}
$referenceProjectRoot = (Resolve-Path -LiteralPath $ReferenceRoot).Path
$referenceSourceProjectRoot = (Resolve-Path -LiteralPath $ReferenceSourceRoot).Path
$clientPath = (Resolve-Path -LiteralPath $ClientExecutable).Path
$cnCdnBundlePath = (Resolve-Path -LiteralPath $CnCdnBundle).Path
$decompiledPath = (Resolve-Path -LiteralPath $DecompiledRoot).Path
$referenceServerRoot = (Resolve-Path -LiteralPath (
    Join-Path $referenceProjectRoot 'resources\server'
)).Path

$routeAudit = Invoke-NodeJsonAudit `
    -ScriptPath (Join-Path $PSScriptRoot 'audit-cn-reference-route-coverage.mjs') `
    -Arguments @('--reference-root', $referenceServerRoot, '--report-only')
$typescriptSourceAudit = Invoke-NodeJsonAudit `
    -ScriptPath (Join-Path $PSScriptRoot 'audit-cn-typescript-source-contract.mjs') `
    -Arguments @('--source-root', $referenceSourceProjectRoot, '--report-only')
$nonGameAudit = Invoke-NodeJsonAudit `
    -ScriptPath (Join-Path $PSScriptRoot 'audit-reference-non-game-surface.mjs') `
    -Arguments @(
        '--reference-root', $referenceProjectRoot,
        '--cn-cdn-bundle', $cnCdnBundlePath,
        '--client-executable', $clientPath
    )
$iosClientAudit = Invoke-IosClientRouteAudit `
    -ScriptPath (Join-Path $PSScriptRoot 'audit-ios-cn-client-route-coverage.ps1') `
    -ExecutablePath $clientPath `
    -ClientSourceRoot $decompiledPath `
    -ReferenceProjectRoot $referenceProjectRoot
$dynamicAudit = Invoke-DynamicReferenceAudit `
    -ScriptPath (Join-Path $PSScriptRoot 'run-cn-reference-differential-local.ps1') `
    -ReferenceProjectRoot $referenceProjectRoot `
    -TimeoutMs $RequestTimeoutMs
$dynamicReport = Get-Content -LiteralPath $dynamicAudit.report -Raw -Encoding UTF8 |
    ConvertFrom-Json -Depth 100
$corpus = Get-Content -LiteralPath (
    Join-Path $PSScriptRoot 'cn-reference-differential-corpus.json'
) -Raw -Encoding UTF8 | ConvertFrom-Json -Depth 100
$routeMatrix = @(New-RouteEvidenceMatrix `
    -RouteAudit $routeAudit `
    -TypeScriptAudit $typescriptSourceAudit `
    -DynamicReport $dynamicReport `
    -Corpus $corpus `
    -IosAudit $iosClientAudit | Sort-Object method, path)
$routeMatrixVerdicts = @($routeMatrix | Group-Object verdict | Sort-Object Name | ForEach-Object {
    [ordered]@{ verdict = $_.Name; count = $_.Count }
})
$routeMatrixFlags = @($routeMatrix | ForEach-Object { $_.evidence_flags } |
    Group-Object | Sort-Object -Property @{ Expression = 'Count'; Descending = $true }, Name | ForEach-Object {
        [ordered]@{ flag = $_.Name; count = $_.Count }
    })
$evidenceGate = New-ReferenceEvidenceGate `
    -ReferenceRoutes @($routeAudit.covered.routes) `
    -RouteMatrix $routeMatrix
$gatePassed = $evidenceGate.passed
$completeReportPath = Join-Path $dynamicAudit.run_root 'complete-audit.json'
$summaryReportPath = Join-Path $dynamicAudit.run_root 'complete-audit-summary.json'

$completeAudit = [ordered]@{
    reference_root = $referenceProjectRoot
    reference_source_root = $referenceSourceProjectRoot
    game_http = [ordered]@{
        routes = $routeAudit.covered.count
        missing = $routeAudit.missing.count
        contract_mismatches = $routeAudit.contracts.mismatched.count
        unresolved_contracts = $routeAudit.contracts.unresolved.count
        unresolved_routes = @($routeAudit.contracts.unresolved.routes)
        typescript_source = [ordered]@{
            summary = $typescriptSourceAudit.summary
            missing = @($typescriptSourceAudit.routes | Where-Object status -eq 'missing')
            mismatched = @($typescriptSourceAudit.routes | Where-Object status -eq 'mismatched')
            unresolved = @($typescriptSourceAudit.routes | Where-Object status -eq 'unresolved')
        }
        dynamic = [ordered]@{
            summary = $dynamicAudit.summary
            runner_exit_code = $dynamicAudit.runner_exit_code
        }
        evidence_matrix = [ordered]@{
            summary = [ordered]@{
                total = $routeMatrix.Count
                verdicts = $routeMatrixVerdicts
                flags = $routeMatrixFlags
            }
            gate = $evidenceGate
            routes = $routeMatrix
        }
    }
    current_ios = [ordered]@{
        executable = $clientPath
        decompiled_root = $decompiledPath
        routes = $iosClientAudit.summary
        decompiled_only = $iosClientAudit.decompiled_only
    }
    sdk = [ordered]@{
        routes = $routeAudit.reference.auxiliary.methodAgnostic.count
        missing = $routeAudit.auxiliary.methodAgnostic.missing.count
        contract_mismatches = $routeAudit.auxiliary.methodAgnostic.contractMismatches.count
    }
    middleware = $routeAudit.auxiliary.middleware
    multiplayer = $routeAudit.auxiliary.multiplayer.comparison.summary
    non_game = $nonGameAudit.summary
    reports = [ordered]@{
        complete = $completeReportPath
        summary = $summaryReportPath
        dynamic = $dynamicAudit.report
        dynamic_run_root = $dynamicAudit.run_root
    }
}
$completeAudit | ConvertTo-Json -Depth 30 |
    Set-Content -LiteralPath $completeReportPath -Encoding UTF8

$compactAudit = [ordered]@{
    audit_status = if ($evidenceGate.passed) {
        'passed'
    } elseif ($evidenceGate.implementation_consistent) {
        'warning'
    } else {
        'failed'
    }
    summary = [ordered]@{
        total = $routeMatrix.Count
        verdicts = $routeMatrixVerdicts
        raw_mismatch_downgraded = @($routeMatrix | Where-Object {
            $_.raw_mismatch.downgraded
        }).Count
        mismatched = $evidenceGate.checks.mismatched.count
        invalid_target_override = $evidenceGate.checks.invalid_target_override.count
        pseudo_matched = $evidenceGate.checks.pseudo_matched.count
        unresolved = $evidenceGate.checks.unresolved.count
    }
    gate = $evidenceGate
    decisive = @($routeMatrix | Where-Object {
        $_.verdict -eq 'mismatched' -or $_.verdict -eq 'target-override'
    } | ForEach-Object {
        [ordered]@{
            method = $_.method
            path = $_.path
            verdict = $_.verdict
            evidence = $_.mismatch_evidence.accepted
        }
    })
    downgraded_raw_mismatches = @($routeMatrix | Where-Object {
        $_.raw_mismatch.downgraded
    } | ForEach-Object {
        [ordered]@{
            method = $_.method
            path = $_.path
            raw_sources = $_.raw_mismatch.sources
            reasons = $_.raw_mismatch.downgrade_reasons
        }
    })
    reports = $completeAudit.reports
}
$compactAudit | ConvertTo-Json -Depth 12 |
    Set-Content -LiteralPath $summaryReportPath -Encoding UTF8
$compactAudit | ConvertTo-Json -Depth 12
if (-not $gatePassed) {
    $blockerSummary = @($evidenceGate.blockers | ForEach-Object {
        '{0}={1}' -f $_.check, $_.count
    }) -join ', '
    if ($evidenceGate.implementation_consistent) {
        [Console]::Error.WriteLine(
            "reference audit evidence warning: $blockerSummary, implementation_consistent=true, report=$completeReportPath"
        )
        exit 0
    }
    [Console]::Error.WriteLine(
        "reference audit gate failed: $blockerSummary, report=$completeReportPath"
    )
    exit 1
}
# //// /汇总完整参考核对 ////
