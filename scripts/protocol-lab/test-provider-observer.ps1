# audience: external
# # test-provider-observer
# 此脚本检查观察器 manifest, Java 隐私约束和探针 ADB 调用.

Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'

$SourceDirectory = [IO.Path]::GetFullPath((Join-Path $PSScriptRoot '..\..\tools\protocol-lab\android-provider-observer'))
$ManifestPath = Join-Path $SourceDirectory 'AndroidManifest.xml'
$JavaSourcePath = Join-Path $SourceDirectory 'src\com\mtl\check\ProviderObserver.java'
$ProbeScriptPath = Join-Path $PSScriptRoot 'probe-provider-observer.ps1'

# //// 验证目标 Provider authority 与导出策略 [@x380kkm 2026-07-28] ////
[xml]$Manifest = Get-Content -LiteralPath $ManifestPath -Raw -Encoding UTF8
$Namespace = New-Object System.Xml.XmlNamespaceManager($Manifest.NameTable)
$Namespace.AddNamespace('android', 'http://schemas.android.com/apk/res/android')
$Provider = $Manifest.SelectSingleNode('/manifest/application/provider', $Namespace)
if ($null -eq $Provider) {
    throw "观察器 manifest 不包含 Provider: $ManifestPath"
}
if ($Manifest.manifest.package -cne 'com.mtl.check') {
    throw "观察器 manifest 包名错误: $($Manifest.manifest.package)"
}
if ($Provider.GetAttribute('authorities', 'http://schemas.android.com/apk/res/android') -cne 'com.mtl.check.DataContentProvider') {
    throw '观察器 manifest authority 错误'
}
if ($Provider.GetAttribute('exported', 'http://schemas.android.com/apk/res/android') -cne 'true') {
    throw '观察器 Provider 必须向客户端导出'
}
# //// /验证目标 Provider authority 与导出策略 ////

# //// 验证 Java 观察器只记录形状且不读取字段值 [@x380kkm 2026-08-03] ////
$Source = Get-Content -LiteralPath $JavaSourcePath -Raw -Encoding UTF8
$RequiredJavaText = @(
    'public Bundle call(String method, String arg, Bundle extras)',
    'public Cursor query(Uri uri, String[] projection, String selection, String[] selectionArgs, String sortOrder)',
    'public Cursor query(Uri uri, String[] projection, Bundle queryArgs, CancellationSignal cancellationSignal)',
    'private static Cursor syntheticCursor()',
    'return new ShapeCursor();',
    'private static final class ShapeCursor extends CursorWrapper',
    'super(new MatrixCursor(new String[0]))',
    'Cursor.FIELD_TYPE_NULL',
    'private static String safeText(String value)'
)
foreach ($RequiredText in $RequiredJavaText) {
    if ($Source.IndexOf($RequiredText, [StringComparison]::Ordinal) -lt 0) {
        throw "观察器 Java 源代码缺少形状取证结构: $RequiredText"
    }
}
$EmptyEntrypoints = [ordered]@{
    'public Bundle call(String method, String arg, Bundle extras)' = 'return null;'
    'public String getType(Uri uri)' = 'return null;'
    'public Uri insert(Uri uri, ContentValues values)' = 'return null;'
    'public int delete(Uri uri, String selection, String[] selectionArgs)' = 'return 0;'
    'public int update(Uri uri, ContentValues values, String selection, String[] selectionArgs)' = 'return 0;'
}
foreach ($Entrypoint in $EmptyEntrypoints.GetEnumerator()) {
    $MethodPattern = [regex]::Escape($Entrypoint.Key) + '\s*\{(?<body>.*?)\n    \}'
    $MethodMatch = [regex]::Match($Source, $MethodPattern, [System.Text.RegularExpressions.RegexOptions]::Singleline)
    if (-not $MethodMatch.Success) {
        throw "观察器源代码缺少入口: $($Entrypoint.Key)"
    }
    $ReturnStatements = [regex]::Matches($MethodMatch.Groups['body'].Value, '\breturn\s+[^;]+;')
    if ($ReturnStatements.Count -ne 1 -or $ReturnStatements[0].Value.Trim() -cne $Entrypoint.Value) {
        throw "观察器入口没有精确返回空结果: $($Entrypoint.Key)"
    }
}
$LegacyQueryPattern = [regex]::Escape('public Cursor query(Uri uri, String[] projection, String selection, String[] selectionArgs, String sortOrder)') + '\s*\{(?<body>.*?)\n    \}'
$ModernQueryPattern = [regex]::Escape('public Cursor query(Uri uri, String[] projection, Bundle queryArgs, CancellationSignal cancellationSignal)') + '\s*\{(?<body>.*?)\n    \}'
foreach ($QueryPattern in @($LegacyQueryPattern, $ModernQueryPattern)) {
    $QueryMatch = [regex]::Match($Source, $QueryPattern, [System.Text.RegularExpressions.RegexOptions]::Singleline)
    if (-not $QueryMatch.Success -or $QueryMatch.Groups['body'].Value -notmatch '\breturn\s+syntheticCursor\(\);') {
        throw '观察器 query 入口没有返回合成 Cursor'
    }
}
if ($Source -match '\b(?:extras|values|queryArgs)\.(?:get|containsKey|keySet|size)\s*\(') {
    throw '观察器源代码读取了 Bundle 或 ContentValues 字段值'
}
if ($Source -match '\buri\s*\.\s*(?!getAuthority\s*\()[A-Za-z_][A-Za-z0-9_]*\s*\(') {
    throw '观察器源代码读取了 URI 路径、查询参数或值'
}
if ($Source -match 'Log\.[A-Za-z]+\([^;]*(?:\+\s*(?:method|arg|extras|projection|selection|selectionArgs|sortOrder|values|queryArgs|cancellationSignal)\b|\b(?:method|arg|extras|projection|selection|selectionArgs|sortOrder|values|queryArgs|cancellationSignal)\s*\+)') {
    throw '观察器日志包含原始调用字段'
}
# //// /验证 Java 观察器只记录形状且不读取字段值 ////

# //// 验证探针使用正确 AIR 入口且只读取观察器日志 [@x380kkm 2026-07-28] ////
$ProbeSource = Get-Content -LiteralPath $ProbeScriptPath -Raw -Encoding UTF8
foreach ($RequiredText in @(
    "`$ClientPackage = 'com.leiting.wf'",
    "`$ClientActivity = 'air.com.leiting.wf.AppEntry'",
    "`$ObserverLogTag = 'StarpointProviderObserver'",
    "`$AdbCommandTimeoutSeconds = 10",
    "`$ApkInstallTimeoutSeconds = 180",
    "`$ResolvedAdbServerPort = Resolve-ProtocolLabAdbServerPort",
    "`$InstallResult = Invoke-ProtocolLabAdb",
    "`$ObserverPackageResult = Invoke-ProtocolLabAdb",
    "`$LogcatClearResult = Invoke-ProtocolLabAdb",
    "`$StartResult = Invoke-ProtocolLabAdb",
    "`$LogcatReadResult = Invoke-ProtocolLabAdb",
    "`$ObserverLines = @(`$LogcatReadResult.Output |",
    "`$LegacyQueryPattern = 'query variant=legacy",
    "`$ModernQueryPattern = 'query variant=modern"
)) {
    if ($ProbeSource.IndexOf($RequiredText, [StringComparison]::Ordinal) -lt 0) {
        throw "观察器探针缺少约束文本: $RequiredText"
    }
}
if ($ProbeSource -match 'Invoke-WebRequest|http://|https://|RequestSetting|token=') {
    throw '观察器探针包含服务器调用或凭据字段'
}
if ($ProbeSource.Contains('& $State.AdbPath', [StringComparison]::Ordinal)) {
    throw '观察器探针仍直接启动未限时的 ADB 进程'
}
$InstallTimeoutCount = [regex]::Matches($ProbeSource, '-CommandArguments\s+@\(''install''.*?-TimeoutSeconds\s+\$ApkInstallTimeoutSeconds', [System.Text.RegularExpressions.RegexOptions]::Singleline).Count
$CommandTimeoutCount = [regex]::Matches($ProbeSource, '-TimeoutSeconds\s+\$AdbCommandTimeoutSeconds').Count
if ($InstallTimeoutCount -ne 1 -or $CommandTimeoutCount -ne 6) {
    throw "观察器探针的 ADB 超时分配不正确: install=$InstallTimeoutCount command=$CommandTimeoutCount"
}
$AdbServerPortCount = [regex]::Matches($ProbeSource, '-AdbServerPort\s+\$ResolvedAdbServerPort\b').Count
if ($AdbServerPortCount -ne 7) {
    throw "观察器探针未向全部 ADB 调用传递状态端口: actual=$AdbServerPortCount expected=7"
}
if ($ProbeSource.IndexOf('观察器日志读取失败', [StringComparison]::Ordinal) -lt 0) {
    throw '观察器探针没有区分日志读取失败和零次调用'
}
# //// /验证探针使用正确 AIR 入口且只读取观察器日志 ////

Write-Output 'test-provider-observer: PASS'
