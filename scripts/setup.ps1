# Installs the local transcription engine (faster-whisper) and pre-downloads a model.
# Usage: powershell -ExecutionPolicy Bypass -File scripts/setup.ps1
#        powershell -ExecutionPolicy Bypass -File scripts/setup.ps1 -Model medium

param(
    [string]$Model = "small",
    [switch]$SkipModel,
    # 預設自動偵測：有 NVIDIA 顯卡就加裝 CUDA/cuDNN（GPU 加速用）。
    # 純 CPU 使用者可用 -CpuOnly 跳過（省約 2 GB 下載）。
    [switch]$Gpu,
    [switch]$CpuOnly
)

$ErrorActionPreference = "Stop"
$root = Split-Path $PSScriptRoot -Parent
$sidecar = Join-Path $root "sidecar"
$venv = Join-Path $sidecar ".venv"
$venvPython = Join-Path $venv "Scripts\python.exe"

Write-Host ""
Write-Host "  Undertone · 本機轉錄引擎安裝" -ForegroundColor Green
Write-Host "  ------------------------------"
Write-Host ""

if (-not (Test-Path $venvPython)) {
    $python = Get-Command python -ErrorAction SilentlyContinue
    if (-not $python) {
        throw "找不到 Python。請先安裝 Python 3.11（https://www.python.org/downloads/），再重新執行此腳本。"
    }

    $version = & python -c "import sys; print('%d.%d' % sys.version_info[:2])"
    Write-Host "  使用 Python $version 建立虛擬環境…"
    if ($version -notmatch '^3\.(10|11|12)$') {
        throw "faster-whisper 目前支援 Python 3.10–3.12，偵測到 $version。請安裝 Python 3.11 後再試。"
    }

    & python -m venv $venv
    if ($LASTEXITCODE -ne 0) {
        throw "建立虛擬環境失敗。"
    }
} else {
    Write-Host "  已找到現有的虛擬環境。"
}

Write-Host "  安裝 Python 套件（第一次約需下載 200 MB）…"
& $venvPython -m pip install --upgrade pip | Out-Null
& $venvPython -m pip install -r (Join-Path $sidecar "requirements.txt")
if ($LASTEXITCODE -ne 0) {
    throw "安裝 Python 套件失敗，請檢查網路連線。"
}

# GPU 加速需要 cuBLAS 12 + cuDNN 9（另約 2 GB）。有 NVIDIA 顯卡就預設安裝，
# 缺少時引擎會自動退回 CPU。-CpuOnly 可強制跳過。
$wantGpu = $false
if ($CpuOnly) {
    Write-Host "  已指定 -CpuOnly，跳過 GPU 套件。"
} elseif ($Gpu) {
    $wantGpu = $true
} else {
    $nvidia = Get-Command nvidia-smi -ErrorAction SilentlyContinue
    $wantGpu = $null -ne $nvidia
    if ($wantGpu) {
        Write-Host "  偵測到 NVIDIA GPU，安裝 CUDA 加速套件…"
    } else {
        Write-Host "  未偵測到 NVIDIA GPU，只安裝 CPU 版本（如需 GPU 請用 -Gpu 重跑）。"
    }
}
if ($wantGpu) {
    Write-Host "  安裝 CUDA/cuDNN 套件（約 2 GB，只需一次）…"
    & $venvPython -m pip install nvidia-cublas-cu12 nvidia-cudnn-cu12 nvidia-cuda-runtime-cu12 nvidia-cuda-nvrtc-cu12
    if ($LASTEXITCODE -ne 0) {
        Write-Host "  警告：GPU 套件安裝失敗，仍可繼續用 CPU。" -ForegroundColor Yellow
    } else {
        # 這裡會匯入 ctranslate2；缺少 cuDNN 時它會把警告寫到 stderr，而
        # $ErrorActionPreference = "Stop" 下重新導向的 stderr 會變成終止錯誤，
        # 所以先放寬設定，改用 $LASTEXITCODE 判斷。路徑用 r'...' 避免 Python 把
        # 反斜線當成跳脫字元（會噴 DeprecationWarning）。
        $previousPreference = $ErrorActionPreference
        $ErrorActionPreference = "Continue"
        try {
            $cudaCount = & $venvPython -c "import sys; sys.path.insert(0, r'$sidecar'); import server; server._enable_bundled_cuda_libs(); import ctranslate2; print(ctranslate2.get_cuda_device_count())" 2>$null
        } finally {
            $ErrorActionPreference = $previousPreference
        }
        if ($LASTEXITCODE -ne 0 -or [string]::IsNullOrWhiteSpace($cudaCount)) {
            Write-Host "  警告：無法偵測 CUDA 裝置，請更新 NVIDIA 驅動後重試；目前仍可用 CPU。" -ForegroundColor Yellow
        } else {
            Write-Host "  ctranslate2 偵測到 CUDA 裝置：$cudaCount 個"
            if ("$cudaCount".Trim() -eq "0") {
                Write-Host "  警告：抓不到 CUDA 裝置，請更新 NVIDIA 驅動後重試；目前仍可用 CPU。" -ForegroundColor Yellow
            }
        }
    }
}

if (-not $SkipModel) {
    Write-Host "  下載 Whisper 模型：$Model …"
    $env:UNDERTONE_MODEL_DIR = Join-Path $env:APPDATA "so.undertone.app\models"
    & $venvPython (Join-Path $sidecar "download_model.py") $Model
    if ($LASTEXITCODE -ne 0) {
        throw "模型下載失敗。你可以稍後在 App 內切換模型，或重新執行此腳本。"
    }
}

Write-Host ""
Write-Host "  完成！接下來執行：" -ForegroundColor Green
Write-Host "    npm install"
Write-Host "    npm run app"
Write-Host ""
Write-Host "  預設快捷鍵：Ctrl + Alt + Space"
Write-Host ""
