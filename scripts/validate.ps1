param(
    [string]$AsioSdk = $env:CPAL_ASIO_DIR,
    [string]$TargetDir = $env:CARGO_TARGET_DIR
)
$ErrorActionPreference = 'Stop'
[Console]::OutputEncoding = [System.Text.Encoding]::UTF8
$projectRoot = Split-Path -Parent $PSScriptRoot
Set-Location -LiteralPath $projectRoot
if ($AsioSdk) { $env:CPAL_ASIO_DIR = $AsioSdk }
if ($TargetDir) { $env:CARGO_TARGET_DIR = $TargetDir }
$env:CARGO_INCREMENTAL = '0'
$logDir = Join-Path $projectRoot 'target\validation'
New-Item -ItemType Directory -Path $logDir -Force | Out-Null
$cargo = (Get-Command cargo -ErrorAction SilentlyContinue).Source
if (-not $cargo) { $cargo = Join-Path $env:USERPROFILE '.cargo\bin\cargo.exe' }
function Run-Check([string]$Name, [string[]]$CargoArgs) {
    $logPath = Join-Path $logDir ($Name + '.log')
    & $cargo @CargoArgs *> $logPath
    if ($LASTEXITCODE -ne 0) {
        Get-Content -LiteralPath $logPath -Tail 70
        throw "$Name failed: see $logPath"
    }
    Write-Output "$Name PASS"
    Select-String -LiteralPath $logPath -Pattern '^test result:|^FFT=|^Pure DSP:' | ForEach-Object { $_.Line }
}
# These checks do not open capture/output streams. Never run ignored live_audio here.
Run-Check 'format' @('fmt','--check')
Run-Check 'clippy-all' @('clippy','--locked','--all-targets','--all-features','--','-D','warnings')
Run-Check 'clippy-core' @('clippy','--locked','--no-default-features','--all-targets','--','-D','warnings')
Run-Check 'test-all' @('test','--locked','--all-features')
Run-Check 'test-core' @('test','--locked','--no-default-features')
Run-Check 'test-wasapi' @('test','--locked','--no-default-features','--features','desktop')
Run-Check 'test-release' @('test','--release','--locked','--all-features')
Run-Check 'build-release' @('build','--release','--locked','--all-features')
Run-Check 'dsp-timing' @('run','--release','--locked','--no-default-features','--example','dsp_timing')
