param([string]$Cargo = 'cargo')
$ErrorActionPreference = 'Stop'
$transferRoot = Split-Path -Parent $PSScriptRoot
$transferOldGui = $env:CCT_GUI_PAYLOAD
$transferOldLoader = $env:CCT_LOADER_PAYLOAD
Push-Location $transferRoot
try {
    $transferMetadata = (& $Cargo metadata --format-version 1 --filter-platform x86_64-pc-windows-gnu --locked) | ConvertFrom-Json
    if ($LASTEXITCODE -ne 0) { throw 'Cargo metadata failed' }
    $transferSdk = $transferMetadata.packages | Where-Object name -eq 'webview2-com-sys' | Select-Object -First 1
    if (-not $transferSdk) { throw 'WebView2 SDK dependency not found' }
    $transferVersion = ($transferMetadata.packages | Where-Object name -eq 'codex-chat-transfer' | Select-Object -First 1).version
    $env:CCT_GUI_PAYLOAD = Join-Path $transferRoot 'target\release\codex-chat-transfer.exe'
    $env:CCT_LOADER_PAYLOAD = Join-Path (Split-Path -Parent $transferSdk.manifest_path) 'x64\WebView2Loader.dll'
    if (-not (Test-Path -LiteralPath $env:CCT_GUI_PAYLOAD)) { throw 'Build the GUI executable first' }
    & $Cargo build --release --locked --manifest-path (Join-Path $transferRoot 'single-exe\Cargo.toml')
    if ($LASTEXITCODE -ne 0) { throw 'Single executable build failed' }
    New-Item -ItemType Directory -Force -Path (Join-Path $transferRoot 'dist') | Out-Null
    $transferOutput = Join-Path $transferRoot ("dist\codex-chat-transfer-single-v" + $transferVersion + '.exe')
    Copy-Item -LiteralPath (Join-Path $transferRoot 'single-exe\target\release\codex-chat-transfer-single.exe') -Destination $transferOutput -Force
    Write-Output $transferOutput
} finally {
    $env:CCT_GUI_PAYLOAD = $transferOldGui
    $env:CCT_LOADER_PAYLOAD = $transferOldLoader
    Pop-Location
}
