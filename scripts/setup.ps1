# Installs the local transcription engine (faster-whisper) and pre-downloads a model.
# Usage: powershell -ExecutionPolicy Bypass -File scripts/setup.ps1
#        powershell -ExecutionPolicy Bypass -File scripts/setup.ps1 -Model medium

param(
    [string]$Model = "small",
    [switch]$SkipModel
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
