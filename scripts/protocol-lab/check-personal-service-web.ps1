# audience: internal
# # check-personal-service-web
# 发布入口校验嵌入页面与 JavaScript 源文件一致.

[CmdletBinding()]
param([Parameter(Mandatory)][string]$RepositoryRoot)

$ErrorActionPreference = 'Stop'
Set-StrictMode -Version Latest

# //// 校验个人服务网页构建产物 [@x380kkm 2026-09-26] ////
& npm --prefix $RepositoryRoot run check:personal-web | Out-Host
if ($LASTEXITCODE -ne 0) { throw 'Personal service web build check failed.' }
# //// /校验个人服务网页构建产物 ////
