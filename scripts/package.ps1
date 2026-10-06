param(
    [ValidateSet('1', '2', '3')]
    [string]$Mode = '3',
    [string]$Cargo = $(if ($env:CCT_CARGO) { $env:CCT_CARGO } else { 'cargo' })
)
$ErrorActionPreference = 'Stop'
$transferRoot = Split-Path -Parent $PSScriptRoot
Push-Location $transferRoot
try {
    if (-not (Get-Command $Cargo -ErrorAction SilentlyContinue)) {
        throw 'Rust Cargo was not found. Install Rust or set CCT_CARGO to cargo.exe.'
    }
    if (-not (Get-Command npm.cmd -ErrorAction SilentlyContinue)) {
        throw 'npm was not found. Install Node.js and reopen the terminal.'
    }
    & npm.cmd ci
    if ($LASTEXITCODE -ne 0) { throw 'npm ci failed' }
    & npm.cmd run build
    if ($LASTEXITCODE -ne 0) { throw 'Frontend build failed' }
    & $Cargo build --release --locked
    if ($LASTEXITCODE -ne 0) { throw 'Rust build failed' }
    if ($Mode -in @('2', '3')) {
        & (Join-Path $PSScriptRoot 'package-portable.ps1') -Cargo $Cargo -SkipBuild
    }
    if ($Mode -in @('1', '3')) {
        & (Join-Path $PSScriptRoot 'package-single.ps1') -Cargo $Cargo
    }
} catch {
    Write-Error $_
    exit 1
} finally {
    Pop-Location
}
