param([string]$Executable, [string[]]$Include)
$ErrorActionPreference = 'Stop'
[Console]::OutputEncoding = [System.Text.Encoding]::UTF8
$projectRoot = Split-Path -Parent $PSScriptRoot
if (-not $Executable) { $Executable = Join-Path $projectRoot 'target\deliverables\AudioAnalyzer.exe' }
if (-not (Test-Path -LiteralPath $Executable)) { throw 'Build/copy the release executable first.' }
$qa = Join-Path $projectRoot 'target\qa'
New-Item -ItemType Directory -Path $qa -Force | Out-Null
$runs = @(
    @{ Name='release-desktop'; Args=@('--demo') },
    @{ Name='release-minimum'; Args=@('--demo','--width','760','--height','640') },
    @{ Name='release-minimum-history'; Args=@('--demo','--width','760','--height','640','--qa-history') },
    @{ Name='release-settings'; Args=@('--demo','--qa-settings') },
    @{ Name='release-settings-analysis'; Args=@('--demo','--qa-settings','--qa-settings-page','1') },
    @{ Name='release-settings-conditions'; Args=@('--demo','--qa-settings','--qa-settings-page','2') },
    @{ Name='release-settings-diagnostics'; Args=@('--demo','--qa-settings','--qa-settings-page','3') },
    @{ Name='release-settings-minimum'; Args=@('--demo','--qa-settings','--width','760','--height','640') },
    @{ Name='release-900'; Args=@('--demo','--height','900') },
    @{ Name='release-zoom150'; Args=@('--demo','--qa-zoom','1.5') },
    @{ Name='release-zoom200'; Args=@('--demo','--qa-settings','--qa-settings-page','2','--qa-zoom','2') },
    @{ Name='release-idle'; Args=@('--qa-idle') },
    @{ Name='release-hold'; Args=@('--demo','--qa-hold') }
)
# Only explicit synthetic/no-input modes. Never starts a hardware input or output.
foreach ($run in $runs) {
    if ($Include -and $run.Name -notin $Include) { continue }
    $screenshot = Join-Path $qa ($run.Name + '.png')
    $arguments = $run.Args + @('--qa-screenshot', ('"' + $screenshot + '"'))
    $stdout = Join-Path $qa ($run.Name + '.out.log')
    $stderr = Join-Path $qa ($run.Name + '.err.log')
    $started = [DateTime]::UtcNow
    $process = Start-Process -FilePath $Executable -ArgumentList $arguments -WindowStyle Hidden -RedirectStandardOutput $stdout -RedirectStandardError $stderr -Wait -PassThru
    Write-Output "$($run.Name) exit=$($process.ExitCode)"
    Get-Content -LiteralPath $stdout
    Get-Content -LiteralPath $stderr
    if ($process.ExitCode -ne 0 -or -not (Test-Path -LiteralPath $screenshot) -or (Get-Item -LiteralPath $screenshot).LastWriteTimeUtc -lt $started.AddSeconds(-1)) {
        throw "$($run.Name) failed or no fresh screenshot; exit=$($process.ExitCode); inspect $stderr"
    }
}
