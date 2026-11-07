# 阶段1 指标验收：体积 / 启动耗时 / 空闲内存 / 空闲 CPU。
# 必须在**已提权**的 PowerShell 里跑：exe 的 manifest 是 requireAdministrator，
# 从非提权会话启动会把 UAC 弹窗等待算进启动耗时，且无法结束上一份实例。
# 用法：powershell -File check_metrics.ps1
param(
    [string]$Target = "aarch64-pc-windows-msvc",
    [int]$IdleSeconds = 5,
    [switch]$KeepRunning
)

$exe = "target\$Target\release\nictool.exe"

function Fail([string]$Message) {
    Write-Host "指标未测出：$Message"
    exit 1
}

if (-not (Test-Path $exe)) { Fail "未找到 $exe，先运行 build.ps1" }

$elevated = ([Security.Principal.WindowsPrincipal][Security.Principal.WindowsIdentity]::GetCurrent()
    ).IsInRole([Security.Principal.WindowsBuiltInRole]::Administrator)
if (-not $elevated) {
    Fail "当前会话未提权：启动耗时会包含 UAC 等待，请在管理员 PowerShell 中重跑"
}

$limits = [ordered]@{
    "exe 体积(MB)"     = 2
    "空闲私有内存(MB)" = 10
    "启动耗时(ms)"     = 300
    "空闲 CPU(%)"      = 1
}

Get-Process nictool -ErrorAction SilentlyContinue | ForEach-Object {
    Write-Host "结束上一份实例 PID $($_.Id)"
    Stop-Process -Id $_.Id -Force
}
Start-Sleep -Milliseconds 200

# 启动耗时 = 进程创建到主窗口句柄可见，而不是「Start-Process 返回」那一刻
$sw = [System.Diagnostics.Stopwatch]::StartNew()
$p = Start-Process $exe -PassThru
$handle = 0
while ($sw.ElapsedMilliseconds -lt 5000) {
    $p.Refresh()
    $handle = $p.MainWindowHandle
    if ($handle -ne 0) { break }
    Start-Sleep -Milliseconds 5
}
$startupMs = $sw.ElapsedMilliseconds
if ($handle -eq 0) { Fail "5 秒内主窗口未出现，后续指标无意义" }

Start-Sleep -Seconds $IdleSeconds
$p.Refresh()
$wsMB = $p.WorkingSet64 / 1MB
$privMB = $p.PrivateMemorySize64 / 1MB

# 单次采样会返回进程累计值，取两次间隔才是「空闲时消耗了多少」
$cpu = 0.0
try {
    $a = (Get-Counter "\Process(nictool*)\% User Time" -ErrorAction Stop).CounterSamples |
        Measure-Object CookedValue -Sum
    Start-Sleep -Seconds 2
    $b = (Get-Counter "\Process(nictool*)\% User Time" -ErrorAction Stop).CounterSamples |
        Measure-Object CookedValue -Sum
    $cpu = [math]::Max(0, $b.Sum - $a.Sum)
}
catch { Write-Host "CPU 采样不可用：$($_.Exception.Message)" }

$exeMB = (Get-Item $exe).Length / 1MB
$rows = @(
    [pscustomobject]@{ 指标 = "exe 体积(MB)";     实测 = [math]::Round($exeMB, 2);   阈值 = $limits["exe 体积(MB)"] }
    [pscustomobject]@{ 指标 = "启动耗时(ms)";     实测 = $startupMs;                  阈值 = $limits["启动耗时(ms)"] }
    [pscustomobject]@{ 指标 = "空闲私有内存(MB)"; 实测 = [math]::Round($privMB, 2);   阈值 = $limits["空闲私有内存(MB)"] }
    [pscustomobject]@{ 指标 = "空闲工作集(MB)";   实测 = [math]::Round($wsMB, 2);     阈值 = "（参考项）" }
    [pscustomobject]@{ 指标 = "空闲 CPU(2s 增量%)"; 实测 = [math]::Round($cpu, 3);    阈值 = $limits["空闲 CPU(%)"] }
)
$rows | Format-Table -AutoSize
Write-Host "内存口径已裁定：按私有内存（进程独占的提交量）判 <10MB；工作集含可被换出的共享页，只作参考。"

if (-not $KeepRunning) {
    Stop-Process -Id $p.Id -Force -ErrorAction SilentlyContinue
}

$failed = @()
if ($exeMB -ge $limits["exe 体积(MB)"])   { $failed += "exe 体积" }
if ($startupMs -ge $limits["启动耗时(ms)"]) { $failed += "启动耗时" }
if ($privMB -ge $limits["空闲私有内存(MB)"]) { $failed += "空闲私有内存" }
if ($cpu -ge $limits["空闲 CPU(%)"])      { $failed += "空闲 CPU" }
if ($failed.Count -gt 0) {
    Write-Host ("未达标：{0}" -f ($failed -join "、"))
    exit 2
}
Write-Host "指标达标"
