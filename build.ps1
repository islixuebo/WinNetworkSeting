# 阶段1 构建脚本：固化 vcvars 环境与 cargo PATH，避免每次手导。
# 用法：powershell -ExecutionPolicy Bypass -File build.ps1
param(
    [string]$Target = "aarch64-pc-windows-msvc",
    [switch]$SkipVcvars
)

# 刻意不设 $ErrorActionPreference=Stop：PS 5.1 下它会把原生命令的 stderr 变成终止错误，
# 连 cargo 的退出码与诊断一起吞掉（阶段1 已踩过）。
function Fail([string]$Message) {
    Write-Host "构建失败：$Message"
    exit 1
}

# rustup 装的 cargo 不在新机默认 PATH 上
$cargoBin = Join-Path $env:USERPROFILE ".cargo\bin"
if (-not (Get-Command cargo -ErrorAction SilentlyContinue)) {
    if (Test-Path (Join-Path $cargoBin "cargo.exe")) {
        $env:PATH = "$cargoBin;$env:PATH"
    }
    else {
        Fail "找不到 cargo，请先安装 Rust（aarch64-pc-windows-msvc 工具链）"
    }
}

if (-not $SkipVcvars) {
    $vs = "C:\Program Files (x86)\Microsoft Visual Studio\2022\BuildTools\VC\Auxiliary\Build\vcvarsall.bat"
    if (-not (Test-Path $vs)) {
        $vs = & "C:\Program Files (x86)\Microsoft Visual Studio\Installer\vswhere.exe" `
            -latest -products * -requires Microsoft.VisualStudio.Component.VC.Tools.x86.x64 `
            -property installationPath
        if ($vs) { $vs = Join-Path $vs "VC\Auxiliary\Build\vcvarsall.bat" }
    }
    if (-not (Test-Path $vs)) {
        Fail "找不到 vcvarsall.bat，需要 VS 2022 Build Tools 的 C++ 工作负载（含 ARM64）"
    }
    # vcvars 同时带来 cl/link 与 INCLUDE（rc.exe 缺 INCLUDE 会报 RC1015），
    # 用 `cmd /c "... && set"` 把它的环境回灌进当前 PowerShell 进程。
    $arch = if ($Target -like "aarch64*") { "arm64" } else { "amd64" }
    foreach ($line in (cmd /c "`"$vs`" $arch && set")) {
        if ($line -match "^(.*?)=(.*)$") { Set-Item -Path "env:$($matches[1])" -Value $matches[2] }
    }
}

$exe = Join-Path "target\$Target\release" "nictool.exe"
# 提权进程会锁住 exe，链接步骤只在替换文件那一刻失败（os error 5），提前给出人话
$locking = Get-Process nictool -ErrorAction SilentlyContinue
if ($locking) {
    Fail "nictool.exe 正在运行（PID $($locking.Id -join ' ')），请先关闭窗口再构建"
}

cargo build --release --target $Target
if ($LASTEXITCODE -ne 0) { exit $LASTEXITCODE }

Write-Host ("产物：{0}（{1:N2} MB）" -f $exe, ((Get-Item $exe).Length / 1MB))
