# Builds a self-contained sidecar executable with PyInstaller.
# The output (sidecar/dist/undertone-sidecar.exe) can be shipped with the app.
# Usage: powershell -ExecutionPolicy Bypass -File scripts/build-sidecar.ps1

$ErrorActionPreference = "Stop"
$root = Split-Path $PSScriptRoot -Parent
$sidecar = Join-Path $root "sidecar"
$venvPython = Join-Path $sidecar ".venv\Scripts\python.exe"

if (-not (Test-Path $venvPython)) {
    throw "找不到 sidecar\.venv。請先執行 scripts/setup.ps1。"
}

Write-Host "安裝 PyInstaller…"
& $venvPython -m pip install --upgrade pyinstaller
if ($LASTEXITCODE -ne 0) {
    throw "PyInstaller 安裝失敗。"
}

# onnxruntime and ctranslate2 ship native DLLs; onefile keeps distribution simple.
# GPU 支援：若 venv 裡有 nvidia CUDA 套件就一起打包（約 +2 GB），
# 這樣安裝版也能用 GPU；沒有的話打包為 CPU 版（缺少 CUDA 時引擎自動退回 CPU）。
$collectArgs = @(
    "--collect-all", "faster_whisper",
    "--collect-all", "ctranslate2",
    "--collect-all", "onnxruntime",
    "--collect-all", "tokenizers",
    "--collect-all", "av"
)
foreach ($pkg in @("nvidia-cublas-cu12", "nvidia-cudnn-cu12", "nvidia-cuda-runtime-cu12", "nvidia-cuda-nvrtc-cu12")) {
    & $venvPython -m pip show $pkg *> $null
    if ($LASTEXITCODE -eq 0) {
        $importName = switch ($pkg) {
            "nvidia-cublas-cu12" { "nvidia.cublas" }
            "nvidia-cudnn-cu12" { "nvidia.cudnn" }
            "nvidia-cuda-runtime-cu12" { "nvidia.cuda_runtime" }
            "nvidia-cuda-nvrtc-cu12" { "nvidia.cuda_nvrtc" }
        }
        $collectArgs += @("--collect-all", $importName)
        Write-Host "  將打包 GPU 套件：$pkg"
    } else {
        Write-Host "  未安裝 $pkg，跳過（打包為 CPU 版）。"
    }
}
& $venvPython -m PyInstaller `
    --noconfirm `
    --clean `
    --onefile `
    --name undertone-sidecar `
    --distpath (Join-Path $sidecar "dist") `
    --workpath (Join-Path $sidecar "build") `
    --specpath $sidecar `
    @collectArgs `
    (Join-Path $sidecar "server.py")

if ($LASTEXITCODE -ne 0) {
    throw "打包失敗。"
}

$exe = Join-Path $sidecar "dist\undertone-sidecar.exe"
if (-not (Test-Path $exe)) {
    throw "打包完成但找不到 $exe。"
}

$sizeMb = [math]::Round((Get-Item $exe).Length / 1MB, 1)
Write-Host ""
Write-Host "完成：$exe（$sizeMb MB）" -ForegroundColor Green
Write-Host "tauri.installer.conf.json 已經把這個檔案映射到 resources/sidecar/，執行 npm run installer 即可產生安裝檔。"
Write-Host "驗證引擎是否可獨立啟動："
Write-Host "  sidecar\dist\undertone-sidecar.exe --port 8757"
Write-Host ""
