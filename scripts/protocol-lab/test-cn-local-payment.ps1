# audience: internal
# # test-cn-local-payment
# 此脚本在独立目录修改真实客户端 SWF, 并从回读源码核对本地订单与界面完成调用.

[CmdletBinding()]
param(
    [Parameter(Mandatory)][string]$InputSwf,
    [Parameter(Mandatory)][string]$WorkingDirectory,
    [Parameter(Mandatory)][string]$FfdecDirectory,
    [string]$ReferenceSwf
)

$ErrorActionPreference = 'Stop'
Set-StrictMode -Version Latest
if ([string]::IsNullOrWhiteSpace($ReferenceSwf)) { $ReferenceSwf = $InputSwf }

# //// 编译并执行真实客户端的付款补丁 [@x380kkm 2026-09-22] ////
if (Test-Path -LiteralPath $WorkingDirectory) { throw '验证输出目录需要独立的新路径.' }
$classes = Join-Path $WorkingDirectory 'classes'
$scripts = Join-Path $WorkingDirectory 'export/scripts'
$patchedSwf = Join-Path $WorkingDirectory 'local-payment.swf'
New-Item -ItemType Directory -Path $classes -Force | Out-Null
$library = Join-Path $FfdecDirectory 'lib/ffdec_lib.jar'
$classPath = $classes + ';' + (Join-Path $FfdecDirectory 'lib/*')
& javac -encoding UTF-8 -cp $library -d $classes `
    (Join-Path $PSScriptRoot 'LocalPaymentAbcPatch.java') (Join-Path $PSScriptRoot 'AbcMethodDigest.java') `
    (Join-Path $PSScriptRoot 'LocalPaymentAbcPatchTest.java')
if ($LASTEXITCODE -ne 0) { throw '付款补丁编译失败.' }
& java -cp $classPath LocalPaymentAbcPatchTest $ReferenceSwf $InputSwf
if ($LASTEXITCODE -ne 0) { throw '付款闭包引用核对失败.' }
& java -cp $classPath LocalPaymentAbcPatch $ReferenceSwf $InputSwf $patchedSwf (Join-Path $WorkingDirectory 'payment.json')
if ($LASTEXITCODE -ne 0) { throw '付款补丁执行失败.' }
& java -jar (Join-Path $FfdecDirectory 'ffdec-cli.jar') -onerror abort `
    -selectclass 'pinball.payment.real.RealPayment,pinball.payment.gbits.GbitsAndroidRealPayment,pinball.remote.payment.finish.PaymentFinishRealRemote,pinball.common.data.payment.PaymentProductLogic,pinball.dialog.stoneBuy.StoneBuyDialogView' `
    -export script (Join-Path $WorkingDirectory 'export') $patchedSwf
if ($LASTEXITCODE -ne 0) { throw '付款补丁回读失败.' }
# //// /编译并执行真实客户端的付款补丁 ////

# //// 核对购买, 结算和回调使用原有客户端流程 [@x380kkm 2026-09-22] ////
function Read-PaymentMethod {
    param([string]$ClassPath, [string]$Method)
    $source = Get-Content -LiteralPath (Join-Path $scripts $ClassPath) -Raw -Encoding UTF8
    $pattern = '(?ms)^(?<indent>[\t ]*)public function ' + [regex]::Escape($Method) + '\([^\r\n]*\)\s*(?::[^\r\n]+)?\r?\n\k<indent>\{.*?^\k<indent>\}'
    $methodMatches = [regex]::Matches($source, $pattern)
    if ($methodMatches.Count -ne 1) { throw "付款方法数量异常: $Method" }
    $methodMatches[0].Value
}

$platform = 'pinball/payment/gbits/GbitsAndroidRealPayment.as'
$purchase = Read-PaymentMethod $platform 'purchase'
if ($purchase -notmatch 'OnQueryPayStatusSuccess\(\)' -or $purchase -match '\.pay\(|getInstance\(') {
    throw '商品购买没有直接调用本地订单完成路径.'
}
$complete = Read-PaymentMethod $platform 'OnQueryPayStatusSuccess'
if ($complete -notmatch 'productId\s*\+\s*":"\s*\+\s*curPurchargeInfo.orderId' `
        -or $complete -notmatch 'payment.startPaymentFinish\(') {
    throw '订单标识或付款完成调用发生变化.'
}
$initialize = Read-PaymentMethod $platform 'initialize'
if ($initialize -notmatch 'PaymentStatus.Supported\(2\)' -or $initialize -match 'getInstance\(') {
    throw '本地商品初始化仍依赖 SDK.'
}
$real = 'pinball/payment/real/RealPayment.as'
$prepared = Read-PaymentMethod $real 'prepare'
if ($prepared -notmatch 'new GbitsAndroidRealPayment\(\)' -or $prepared -notmatch 'platformPayment = Option.Some\(') {
    throw '本地商品购买没有注册付款处理器.'
}
foreach ($method in @('prepare', 'getPurchasesHandler')) {
    if ((Read-PaymentMethod $real $method) -match 'getInstance\(') { throw "本地付款仍访问 SDK: $method" }
}
$recovery = Read-PaymentMethod $real 'getPurchasesHandler'
if ($recovery -notmatch '\.queryUnFinishOrder\(' -or $recovery -notmatch 'PaymentPlayerLoadedInput.Finished') {
    throw '本地订单恢复没有接入玩家载入流程.'
}
$finish = 'pinball/remote/payment/finish/PaymentFinishRealRemote.as'
$constructor = Read-PaymentMethod $finish 'PaymentFinishRealRemote'
if ($constructor -match 'getInstance\(|getDeviceInfo\(' -or $constructor -notmatch 'startUserRequestDetail\("payment/finish"') {
    throw '付款结算请求仍依赖 SDK 或没有调用本地服务.'
}
$callback = Read-PaymentMethod $finish 'successHandler'
if ($callback -notmatch 'SectionCommand.PaymentFinish\(PaymentFinishInput.Finished' -or $callback -notmatch '_successHandler\(\)') {
    throw '付款完成响应没有通知原有界面和消费回调.'
}
$product = 'pinball/common/data/payment/PaymentProductLogic.as'
$buyable = Read-PaymentMethod $product 'canBeBought'
if ($buyable -notmatch 'return StoneBuyableState.Possible;' -or $buyable -match 'ClientError|purchased_times|get_endTime|canBeBoughtForPremiumLoginBonus') {
    throw '本地商品仍受外部支付日期或限购条件阻断.'
}
if ((Read-PaymentMethod $product 'isDisplayable') -notmatch 'return true;') { throw '本地商品可见性不符合预期.' }
if ((Read-PaymentMethod $product 'isWithinPeriod') -notmatch 'return period[.]isWithin') { throw '商品时间窗口没有使用客户端原有判断.' }
Write-Output '真实 SWF 的商品购买直接提交本地订单完成, 并保留余额更新和界面回调.'
# //// /核对购买, 结算和回调使用原有客户端流程 ////
