param(
    [string]$Cargo = 'cargo',
    [switch]$SkipBuild
)
$ErrorActionPreference = 'Stop'
$transferRoot = Split-Path -Parent $PSScriptRoot
Push-Location $transferRoot
try {
    if (-not $SkipBuild) {
        & npm ci
        if ($LASTEXITCODE -ne 0) { throw 'npm ci failed' }
        & npm run build
        if ($LASTEXITCODE -ne 0) { throw 'Frontend build failed' }
        & $Cargo build --release --locked
        if ($LASTEXITCODE -ne 0) { throw 'Rust build failed' }
    }
    $transferExe = Join-Path $transferRoot 'target\release\codex-chat-transfer.exe'
    $transferBytes = [System.IO.File]::ReadAllBytes($transferExe)
    $transferPe = [BitConverter]::ToInt32($transferBytes, 0x3c)
    if ([BitConverter]::ToUInt16($transferBytes, $transferPe + 4) -ne 0x8664) {
        throw 'This script packages Windows x64 binaries only'
    }
    if ([BitConverter]::ToUInt16($transferBytes, $transferPe + 24 + 68) -ne 2) {
        throw 'The executable is not a Windows GUI binary'
    }
    $transferMetadataRaw = & $Cargo metadata --format-version 1 --filter-platform x86_64-pc-windows-gnu --locked
    if ($LASTEXITCODE -ne 0) { throw 'Cargo metadata failed' }
    $transferMetadata = $transferMetadataRaw | ConvertFrom-Json
    $transferSdk = $transferMetadata.packages | Where-Object name -eq 'webview2-com-sys' | Select-Object -First 1
    if (-not $transferSdk) { throw 'WebView2 SDK dependency not found' }
    $transferLoader = Join-Path (Split-Path -Parent $transferSdk.manifest_path) 'x64\WebView2Loader.dll'
    $transferPackageVersion=($transferMetadata.packages|Where-Object name -eq 'codex-chat-transfer'|Select-Object -First 1).version
    $transferOutput = Join-Path $transferRoot ("dist\portable-v" + $transferPackageVersion)
    New-Item -ItemType Directory -Force -Path $transferOutput | Out-Null
    Copy-Item -LiteralPath $transferExe -Destination (Join-Path $transferOutput 'codex-chat-transfer.exe') -Force
    Copy-Item -LiteralPath (Join-Path $transferRoot 'target\release\codex-chat-transfer-cli.exe') -Destination (Join-Path $transferOutput 'codex-chat-transfer-cli.exe') -Force
    Copy-Item -LiteralPath $transferLoader -Destination (Join-Path $transferOutput 'WebView2Loader.dll') -Force
    Copy-Item -LiteralPath (Join-Path $transferRoot 'README.md') -Destination (Join-Path $transferOutput 'README.md') -Force
    Copy-Item -LiteralPath (Join-Path $transferRoot 'LICENSE') -Destination (Join-Path $transferOutput 'LICENSE') -Force
    $transferZip = Join-Path $transferRoot ("dist\codex-chat-transfer-windows-portable-x64-v" + $transferPackageVersion + '.zip')
    Compress-Archive -LiteralPath (Join-Path $transferOutput 'codex-chat-transfer.exe'), (Join-Path $transferOutput 'codex-chat-transfer-cli.exe'), (Join-Path $transferOutput 'WebView2Loader.dll'), (Join-Path $transferOutput 'README.md'), (Join-Path $transferOutput 'LICENSE') -DestinationPath $transferZip -Force
    Write-Output $transferZip
} finally {
    Pop-Location
}
