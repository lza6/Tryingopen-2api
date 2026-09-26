#requires -Version 7.0
<#
.SYNOPSIS
    tryingopen-2api 本地网关压测脚本（可重复运行，默认不消耗上游配额）。

.DESCRIPTION
    并发探测公开端点 /healthz、/v1/models、/api/proxies，以及（可选带 API key 时）/metrics。
    默认绝不调用 /v1/chat/completions；只有显式使用 -StressChat 时才做 3 次非流式
    对话请求（会消耗 tryingopen.com 上游配额，本机每 24h UTC 约 20 次硬限流）。

    输出：
      1) 人类可读摘要
      2) 一行 JSON 基线，写入 scripts/bench/results-YYYYMMDD.json
    退出码：有 FAIL 返回 1，否则 0。

.PARAMETER BaseUrl
    网关基础地址，默认 http://127.0.0.1:47831
.PARAMETER Concurrency
    并发数，默认 5
.PARAMETER Requests
    每个端点的请求总数，默认 20
.PARAMETER ApiKey
    API key；提供则额外压测需要鉴权的 /metrics，否则跳过鉴权端点
.PARAMETER StressChat
    开关；开启时只跑 3 次 /v1/chat/completions 非流式（消耗上游配额）
.PARAMETER ChatModel
    -StressChat 时的模型名，默认 gpt-4o-mini
#>
[CmdletBinding()]
param(
    [string]$BaseUrl    = "http://127.0.0.1:47831",
    [int]$Concurrency   = 5,
    [int]$Requests      = 20,
    [string]$ApiKey     = "",
    [switch]$StressChat,
    [string]$ChatModel  = "gpt-4o-mini"
)

Set-StrictMode -Version Latest
$ErrorActionPreference = "Stop"

$here = $PSScriptRoot
if (-not $here) { $here = Split-Path -Parent $MyInvocation.MyCommand.Path }
$scriptDir = Join-Path $here ".."
$resultsDir = Join-Path $here "results"
if (-not (Test-Path $resultsDir)) { New-Item -ItemType Directory -Path $resultsDir | Out-Null }

$today = Get-Date -Format "yyyyMMdd"
$resultFile = Join-Path $resultsDir ("results-" + $today + ".json")
$ts = (Get-Date).ToString("o")
$exitCode = 0
$hasFail = $false

$endpoints = @(
    @{ Name = "healthz";       Method = "GET";  Path = "/healthz";        Auth = $false },
    @{ Name = "v1_models";     Method = "GET";  Path = "/v1/models";      Auth = $false },
    @{ Name = "api_proxies";   Method = "GET";  Path = "/api/proxies";    Auth = $true }
)
if ($ApiKey) {
    $endpoints += @{ Name = "metrics"; Method = "GET"; Path = "/metrics"; Auth = $true }
}

function Invoke-OneRequest {
    param(
        [string]$Method,
        [string]$Url,
        [string]$ApiKey,
        [bool]$Auth
    )
    $sw = [System.Diagnostics.Stopwatch]::StartNew()
    $status = -1
    $err = ""
    try {
        $headers = @{}
        if ($Auth) { $headers["Authorization"] = "Bearer " + $ApiKey }
        $params = @{ Uri = $Url; Method = $Method; Headers = $headers; TimeoutSec = 30; UseBasicParsing = $true }
        if ($Method -eq "POST") {
            $params["ContentType"] = "application/json"
            $params["Body"] = "{}"
        }
        $resp = Invoke-WebRequest @params
        $status = [int]$resp.StatusCode
    } catch {
        $err = $_.Exception.Message
        if ($_.Exception.Response) {
            try { $status = [int]$_.Exception.Response.StatusCode } catch { $status = -1 }
        } elseif ($_.Exception.Response -and $_.Exception.Response.StatusCode) {
            $status = [int]$_.Exception.Response.StatusCode
        }
    }
    $sw.Stop()
    [pscustomobject]@{
        Status = $status
        Ms     = $sw.Elapsed.TotalMilliseconds
        Error  = $err
    }
}

function Get-Stat {
    param([double[]]$Ms)
    if ($Ms.Count -eq 0) { return [pscustomobject]@{ P50 = 0; P95 = 0; Max = 0 } }
    $sorted = @($Ms | Sort-Object)
    $p50 = $sorted[[math]::Floor(($sorted.Count - 1) * 0.50)]
    $p95 = $sorted[[math]::Floor(($sorted.Count - 1) * 0.95)]
    $max = $sorted[$sorted.Count - 1]
    [pscustomobject]@{ P50 = [math]::Round($p50, 1); P95 = [math]::Round($p95, 1); Max = [math]::Round($max, 1) }
}

Write-Host ""
Write-Host "=== tryingopen-2api 压测 ==="
Write-Host ("BaseUrl: " + $BaseUrl + " | Concurrency: " + $Concurrency + " | Requests/端点: " + $Requests + " | 时间: " + (Get-Date).ToString("yyyy-MM-dd HH:mm:ss"))
if ($ApiKey) { Write-Host "ApiKey: 已提供（将压测 /metrics）" } else { Write-Host "ApiKey: 未提供（跳过鉴权端点）" }
if ($StressChat) {
    Write-Host "警告：-StressChat 已开启，将发起 3 次 /v1/chat/completions 非流式请求，这会消耗 tryingopen.com 上游配额（每 24h UTC 约 20 次）。"
} else {
    Write-Host "安全模式：不调用 /v1/chat/completions，不消耗上游配额。"
}

$endpointResults = @()

foreach ($ep in $endpoints) {
    Write-Host ""
    Write-Host ("--- 端点 " + $ep.Name + " (" + $ep.Method + " " + $ep.Path + ") ---")
    $per = [math]::Max(1, [math]::Min($Concurrency, $Requests))
    $batches = [math]::Ceiling($Requests / $per)
    $all = New-Object System.Collections.Generic.List[object]

    for ($b = 0; $b -lt $batches; $b++) {
        $n = $per
        if (($b + 1) -eq $batches) { $n = $Requests - ($b * $per) }
        $inputs = 1..$n | ForEach-Object { $_ }
        $jobs = $inputs | ForEach-Object -Parallel {
            $sw = [System.Diagnostics.Stopwatch]::StartNew()
            $status = -1
            $err = ""
            $url = $using:BaseUrl + $using:ep.Path
            $headers = @{}
            if ($using:ep.Auth) { $headers["Authorization"] = "Bearer " + $using:ApiKey }
            try {
                $p = @{ Uri = $url; Method = $using:ep.Method; Headers = $headers; TimeoutSec = 30; UseBasicParsing = $true }
                if ($using:ep.Method -eq "POST") { $p["ContentType"] = "application/json"; $p["Body"] = "{}" }
                $resp = Invoke-WebRequest @p
                $status = [int]$resp.StatusCode
            } catch {
                $err = $_.Exception.Message
                try { $status = [int]$_.Exception.Response.StatusCode } catch { $status = -1 }
            }
            $sw.Stop()
            [pscustomobject]@{
                Status = $status
                Ms     = $sw.Elapsed.TotalMilliseconds
                Error  = $err
            }
        } -ThrottleLimit $per
        foreach ($j in $jobs) { $all.Add($j) }
    }

    $statuses = @($all | ForEach-Object { $_.Status })
    $ms = @($all | ForEach-Object { [double]$_.Ms })
    $fails = @($all | Where-Object { $_.Status -lt 200 -or $_.Status -ge 300 })
    $ok = $all.Count - $fails.Count
    $dur = (($ms | Measure-Object -Maximum).Maximum) / 1000.0
    $dur = if ($dur -gt 0) { $dur } else { 1e-9 }
    $rps = $all.Count / $dur
    $stat = Get-Stat -Ms $ms
    $codes = ($statuses | Group-Object | Sort-Object Name | ForEach-Object { $_.Name + "x" + $_.Count }) -join " "

    $isFail = $fails.Count -gt 0
    if ($isFail) { $hasFail = $true }

    Write-Host ("  完成: " + $all.Count + " | 成功: " + $ok + " | FAIL: " + $fails.Count + " | 状态码: " + $codes)
    Write-Host ("  耗时 ms: p50=" + $stat.P50 + " p95=" + $stat.P95 + " max=" + $stat.Max + " | 吞吐: " + [math]::Round($rps, 2) + " req/s")
    if ($isFail) {
        $sample = ($fails | Select-Object -First 3 | ForEach-Object { "status=" + $_.Status + " err=" + $_.Error }) -join " | "
        Write-Host ("  失败样例: " + $sample)
    }

    $endpointResults += [pscustomobject]@{
        Endpoint   = $ep.Name
        Path       = $ep.Path
        Method     = $ep.Method
        Requests   = $all.Count
        Succeeded  = $ok
        Failed     = $fails.Count
        StatusCodes = $codes
        P50Ms      = $stat.P50
        P95Ms      = $stat.P95
        MaxMs      = $stat.Max
        Rps        = [math]::Round($rps, 2)
        Fail       = $isFail
    }
}

if ($StressChat) {
    Write-Host ""
    Write-Host "--- StressChat: 3 次非流式 /v1/chat/completions（消耗上游配额）---"
    $chatRes = @()
    for ($i = 0; $i -lt 3; $i++) {
        $sw = [System.Diagnostics.Stopwatch]::StartNew()
        $chatStatus = -1
        $chatErr = ""
        try {
            $body = @{
                model = $ChatModel
                messages = @(@{ role = "user"; content = "ping" })
                stream = $false
            } | ConvertTo-Json -Depth 6
            $hdrs = @{}
            if ($ApiKey) { $hdrs["Authorization"] = "Bearer " + $ApiKey }
            $resp = Invoke-WebRequest -Uri ($BaseUrl + "/v1/chat/completions") -Method Post -Headers $hdrs -ContentType "application/json" -Body $body -TimeoutSec 180 -UseBasicParsing
            $chatStatus = [int]$resp.StatusCode
        } catch {
            $chatErr = $_.Exception.Message
            try { $chatStatus = [int]$_.Exception.Response.StatusCode } catch { $chatStatus = -1 }
        }
        $sw.Stop()
        $chatRes += [pscustomobject]@{ Status = $chatStatus; Ms = [math]::Round($sw.Elapsed.TotalMilliseconds, 1); Error = $chatErr }
        Write-Host ("  第 " + ($i + 1) + " 次: status=" + $chatStatus + " " + [math]::Round($sw.Elapsed.TotalMilliseconds, 1) + "ms")
        if ($chatErr) { Write-Host ("    err: " + $chatErr) }
    }
    $chatFail = @($chatRes | Where-Object { $_.Status -lt 200 -or $_.Status -ge 300 }).Count
    if ($chatFail -gt 0) { $hasFail = $true }
    $endpointResults += [pscustomobject]@{
        Endpoint    = "chat_completions"
        Path        = "/v1/chat/completions"
        Method      = "POST"
        Requests    = $chatRes.Count
        Succeeded   = $chatRes.Count - $chatFail
        Failed      = $chatFail
        StatusCodes = (($chatRes | ForEach-Object { $_.Status }) -join "x")
        P50Ms       = 0
        P95Ms       = 0
        MaxMs       = 0
        Rps         = 0
        Fail        = ($chatFail -gt 0)
    }
}

$rustVersion = ""
try { $rustVersion = (rustc --version 2>$null | Out-String).Trim() } catch { $rustVersion = "rustc unavailable" }

$json = [pscustomobject]@{
    tool        = "tryingopen-2api bench.ps1"
    timestamp   = $ts
    base_url    = $BaseUrl
    concurrency = $Concurrency
    requests    = $Requests
    stress_chat = [bool]$StressChat
    rust        = $rustVersion
    endpoints   = $endpointResults
} | ConvertTo-Json -Depth 8

$json | Out-File -FilePath $resultFile -Encoding utf8

Write-Host ""
Write-Host "=== 摘要 ==="
Write-Host ("总端点: " + $endpointResults.Count + " | 总体 FAIL: " + $hasFail)
Write-Host ("JSON 基线已写入: " + $resultFile)
Write-Host $json

if ($hasFail) { exit 1 }
exit 0

