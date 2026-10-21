param([string]$Executable, [string[]]$Include)
$ErrorActionPreference='Stop'
[Console]::OutputEncoding=[System.Text.Encoding]::UTF8
$projectRoot=Split-Path -Parent $PSScriptRoot
if(-not $Executable){$Executable=Join-Path $projectRoot 'target\deliverables\AudioVisualizer.exe'}
$qa=Join-Path $projectRoot 'target\visualizer\after'
New-Item -ItemType Directory -Path $qa -Force | Out-Null
$runs=@(
    @{Name='visualizer-3d';Args=@('--demo','--qa-duration','10','--qa-delay','12')},
    @{Name='visualizer-2d';Args=@('--demo','--qa-2d','--qa-duration','10','--qa-delay','12','--qa-cursor')},
    @{Name='visualizer-orbit-reference';Args=@('--demo','--qa-orbit','--qa-reference','--qa-cursor','--qa-duration','10','--qa-delay','12')},
    @{Name='visualizer-minimum';Args=@('--demo','--width','760','--height','640','--qa-duration','10','--qa-delay','12')},
    @{Name='visualizer-minimum-2d';Args=@('--demo','--width','760','--height','640','--qa-2d','--qa-duration','10','--qa-delay','12')},
    @{Name='visualizer-range';Args=@('--demo','--qa-2d','--qa-minimum-dbfs','-80','--qa-duration','10','--qa-delay','8')},
    @{Name='visualizer-reference-detail';Args=@('--demo','--qa-reference','--qa-cursor','--qa-history','--qa-duration','10','--qa-delay','8')},
    @{Name='visualizer-inspector';Args=@('--demo','--qa-cursor','--qa-history','--qa-duration','10','--qa-delay','8')},
    @{Name='visualizer-hold';Args=@('--demo','--qa-hold','--qa-cursor','--qa-reference','--qa-delay','8')},
    @{Name='visualizer-idle';Args=@('--qa-idle')}
)
foreach($run in $runs){
    if($Include -and $run.Name -notin $Include){continue}
    $png=Join-Path $qa ($run.Name+'.png')
    $stdout=Join-Path $qa ($run.Name+'.out.log')
    $stderr=Join-Path $qa ($run.Name+'.err.log')
    $started=[DateTime]::UtcNow
    $arguments=$run.Args+@('--qa-screenshot',('"'+$png+'"'))
    $process=Start-Process -FilePath $Executable -ArgumentList $arguments -WindowStyle Hidden -RedirectStandardOutput $stdout -RedirectStandardError $stderr -Wait -PassThru
    Write-Output "$($run.Name) exit=$($process.ExitCode)"
    Get-Content -LiteralPath $stdout
    Get-Content -LiteralPath $stderr
    if($process.ExitCode -ne 0 -or -not(Test-Path -LiteralPath $png) -or (Get-Item -LiteralPath $png).LastWriteTimeUtc -lt $started.AddSeconds(-1)){throw "Native visualizer QA failed: $($run.Name)"}
}
