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
& $venvPython -m PyInstaller `
    --noconfirm `
    --clean `
    --onefile `
    --name undertone-sidecar `
    --distpath (Join-Path $sidecar "dist") `
    --workpath (Join-Path $sidecar "build") `
    --specpath $sidecar `
    --collect-all faster_whisper `
    --collect-all ctranslate2 `
    --collect-all onnxruntime `
    --collect-all tokenizers `
    --collect-all av `
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
