param(
    [string]$TargetDirectory = (Join-Path $env:TEMP 'AudioVisualizer-beta2-build'),
    [string]$LibClangPath = 'C:\Program Files\LLVM\bin',
    [string]$Cargo = 'cargo'
)
$ErrorActionPreference = 'Stop'
$projectRoot = Split-Path -Parent $PSScriptRoot
$targetRoot = [IO.Path]::GetFullPath($TargetDirectory)
if ($targetRoot -match '[^\x00-\x7F]') {
    throw 'Choose an ASCII TargetDirectory for the ASIO Windows build tools.'
}
$sourceSdk = Join-Path $projectRoot 'ASIO-SDK'
$dependencyRoot = Join-Path $projectRoot 'dependencies'
if (-not (Test-Path -LiteralPath (Join-Path $sourceSdk 'common\asio.cpp')) -or
    -not (Test-Path -LiteralPath $dependencyRoot)) {
    throw 'Extract the complete corresponding-source release ZIP before building.'
}
if (-not (Test-Path -LiteralPath $LibClangPath)) {
    throw 'Pass LibClangPath for an installed LLVM/libclang.'
}
New-Item -ItemType Directory -Path $targetRoot -Force | Out-Null
$sdkBuildCopy = Join-Path $targetRoot 'asio-sdk'
foreach ($sourceFile in Get-ChildItem -LiteralPath $sourceSdk -File -Recurse) {
    $relative = $sourceFile.FullName.Substring($sourceSdk.Length).TrimStart('\','/')
    $destination = Join-Path $sdkBuildCopy $relative
    if (Test-Path -LiteralPath $destination) {
        if ((Get-FileHash -LiteralPath $destination).Hash -ne (Get-FileHash -LiteralPath $sourceFile.FullName).Hash) {
            throw "An existing SDK build copy differs; choose a fresh TargetDirectory: $relative"
        }
    } else {
        New-Item -ItemType Directory -Path (Split-Path -Parent $destination) -Force | Out-Null
        Copy-Item -LiteralPath $sourceFile.FullName -Destination $destination
    }
}
$names = @('CPAL_ASIO_DIR','CARGO_TARGET_DIR','LIBCLANG_PATH','CARGO_INCREMENTAL','CARGO_ENCODED_RUSTFLAGS','CARGO_NET_OFFLINE')
$previous = @{}
foreach ($name in $names) { $previous[$name] = [Environment]::GetEnvironmentVariable($name, 'Process') }
try {
    $env:CPAL_ASIO_DIR = $sdkBuildCopy
    $env:CARGO_TARGET_DIR = Join-Path $targetRoot 'target'
    $env:LIBCLANG_PATH = $LibClangPath
    $env:CARGO_INCREMENTAL = '0'
    $env:CARGO_NET_OFFLINE = 'true'
    $mappings = @(
        @($env:USERPROFILE, '/build/user'),
        @($projectRoot, '/build/Audio-Analyzer'),
        @($dependencyRoot, '/build/dependencies'),
        @($sdkBuildCopy, '/build/ASIO-SDK'),
        @($targetRoot, '/build/output')
    )
    $flags = foreach ($mapping in $mappings) {
        if ($mapping[0]) {
            "--remap-path-prefix=$($mapping[0])=$($mapping[1])"
            "--remap-path-prefix=$($mapping[0].Replace('\','/'))=$($mapping[1])"
        }
    }
    $env:CARGO_ENCODED_RUSTFLAGS = $flags -join [char]31
    Push-Location $projectRoot
    try {
        & $Cargo build --release --all-features --locked --offline --config '.cargo/offline-source.toml'
        if ($LASTEXITCODE -ne 0) { throw "Cargo build failed: $LASTEXITCODE" }
        Write-Output ('Built executable: ' + (Join-Path $env:CARGO_TARGET_DIR 'release\audio_analyzer.exe'))
    } finally { Pop-Location }
} finally {
    foreach ($name in $names) { [Environment]::SetEnvironmentVariable($name, $previous[$name], 'Process') }
}
