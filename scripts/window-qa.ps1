param(
    [Parameter(Mandatory=$true)][string]$Executable,
    [switch]$Capture,
    [string]$Name='window-cycle',
    [string[]]$AppArguments=@()
)
$ErrorActionPreference='Stop'
[Console]::OutputEncoding=[Text.UTF8Encoding]::new()
$projectRoot=Split-Path -Parent $PSScriptRoot
$qa=Join-Path $projectRoot 'target\window-qa'
New-Item -ItemType Directory -Path $qa -Force | Out-Null
# A native regression fixture, scoped to the process it launches. No global input,
# foreground activation, system settings, microphone or speaker operations.
if(-not ('AnalyzerWindowQa' -as [type])) { Add-Type -TypeDefinition @'
using System;
using System.Text;
using System.Collections.Generic;
using System.Runtime.InteropServices;
public static class AnalyzerWindowQa {
    [StructLayout(LayoutKind.Sequential)] public struct RECT { public int Left,Top,Right,Bottom; }
    [DllImport("user32.dll")] public static extern uint GetWindowThreadProcessId(IntPtr h, out uint p);
    [DllImport("user32.dll")] public static extern bool ShowWindowAsync(IntPtr h,int c);
    [DllImport("user32.dll")] public static extern bool GetClientRect(IntPtr h,out RECT r);
    [DllImport("user32.dll", SetLastError=true)] public static extern bool SetWindowPos(IntPtr h,IntPtr after,int x,int y,int w,int v,uint f);
    [DllImport("user32.dll", SetLastError=true)] public static extern bool PostMessage(IntPtr h,uint message,IntPtr w,IntPtr l);
    delegate bool EnumProc(IntPtr h,IntPtr l);
    [DllImport("user32.dll")] static extern bool EnumWindows(EnumProc p,IntPtr l);
    [DllImport("user32.dll",CharSet=CharSet.Unicode)] static extern int GetWindowText(IntPtr h,StringBuilder b,int n);
    [DllImport("user32.dll",CharSet=CharSet.Unicode)] static extern int GetClassName(IntPtr h,StringBuilder b,int n);
    public class WindowInfo { public IntPtr Handle; public string Title; public string ClassName; }
    public static WindowInfo[] ForProcess(int pid) {
        var items=new List<WindowInfo>();
        EnumWindows((h,l)=> {
            uint owner; GetWindowThreadProcessId(h,out owner);
            if(owner==(uint)pid) {
                var title=new StringBuilder(512); var cls=new StringBuilder(256);
                GetWindowText(h,title,title.Capacity); GetClassName(h,cls,cls.Capacity);
                items.Add(new WindowInfo { Handle=h,Title=title.ToString(),ClassName=cls.ToString() });
            }
            return true;
        },IntPtr.Zero);
        return items.ToArray();
    }
}
'@
}
$png=Join-Path $qa ($Name+'.png')
$argsList=@('--demo')+$AppArguments
if($Capture) { $argsList+=@('--qa-hold','--qa-screenshot',('"'+$png+'"')) }
$started=[DateTime]::UtcNow
$clock=[Diagnostics.Stopwatch]::StartNew()
$p=Start-Process -FilePath $Executable -ArgumentList $argsList -WindowStyle Hidden -PassThru -RedirectStandardOutput (Join-Path $qa ($Name+'.out.log')) -RedirectStandardError (Join-Path $qa ($Name+'.err.log'))
$window=[IntPtr]::Zero
while($clock.Elapsed.TotalSeconds -lt 10 -and -not $p.HasExited) {
    $p.Refresh()
    $windows=@([AnalyzerWindowQa]::ForProcess($p.Id))
    $renderWindows=@($windows | Where-Object { $_.ClassName -ne 'ConsoleWindowClass' -and ($_.Title.StartsWith('Audio Analyzer') -or $_.Title.StartsWith('Audio Visualizer')) })
    if($renderWindows.Count -gt 1) { throw 'More than one analyzer render window; refusing ambiguous target' }
    if($renderWindows.Count -eq 1) { $window=$renderWindows[0].Handle; break }
    Start-Sleep -Milliseconds 50
}
if($window -eq [IntPtr]::Zero) { throw 'No window returned by the launched process' }
$windows | ForEach-Object { Write-Output "observed window pid=$($p.Id) class=$($_.ClassName) title=$($_.Title) handle=$($_.Handle)" }
Write-Output "selected native render window handle=$window"
[uint32]$ownerPid=0
[void][AnalyzerWindowQa]::GetWindowThreadProcessId($window,[ref]$ownerPid)
if($ownerPid -ne $p.Id) { throw 'Refusing to manipulate a window outside the launched process' }
function Wait-Until([double]$Seconds) {
    while($clock.Elapsed.TotalSeconds -lt $Seconds -and -not $p.HasExited) { Start-Sleep -Milliseconds 25; $p.Refresh() }
}
function Report-Client([string]$Stage) {
    $r=New-Object AnalyzerWindowQa+RECT
    [void][AnalyzerWindowQa]::GetClientRect($window,[ref]$r)
    Write-Output "$Stage pid=$($p.Id) client=$($r.Right-$r.Left)x$($r.Bottom-$r.Top) elapsed=$([math]::Round($clock.Elapsed.TotalSeconds,2)) exited=$($p.HasExited)"
}
[void][AnalyzerWindowQa]::ShowWindowAsync($window,4) # show without activation
Wait-Until 1.0
foreach($size in @(@(800,680),@(1500,1050),@(1000,760),@(1440,1000))) {
    if($p.HasExited) { break }
    if(-not [AnalyzerWindowQa]::SetWindowPos($window,[IntPtr]::Zero,0,0,$size[0],$size[1],0x16)) { throw 'Resize failed' }
    Start-Sleep -Milliseconds 180
    Report-Client 'resize'
}
Wait-Until 3.0
if(-not $p.HasExited) { [void][AnalyzerWindowQa]::ShowWindowAsync($window,6) }
Start-Sleep -Milliseconds 150
Report-Client 'minimize-1'
Wait-Until 3.6
if(-not $p.HasExited) { [void][AnalyzerWindowQa]::ShowWindowAsync($window,9) }
Wait-Until 4.65
if(-not $p.HasExited) { [void][AnalyzerWindowQa]::ShowWindowAsync($window,6) }
Start-Sleep -Milliseconds 150
Report-Client 'minimize-at-capture-deadline'
Wait-Until 6.2
if(-not $p.HasExited) { [void][AnalyzerWindowQa]::ShowWindowAsync($window,9) }
Start-Sleep -Milliseconds 350
Report-Client 'restore'
if(-not $Capture -and -not $p.HasExited) {
    Start-Sleep -Milliseconds 700
    [void][AnalyzerWindowQa]::GetWindowThreadProcessId($window,[ref]$ownerPid)
    if($ownerPid -ne $p.Id) { throw 'Window ownership changed before close' }
    $posted=[AnalyzerWindowQa]::PostMessage($window,0x10,[IntPtr]::Zero,[IntPtr]::Zero)
    Write-Output "WM_CLOSE posted=$posted pid=$($p.Id)"
}
if(-not $p.WaitForExit(15000)) { throw "QA window did not close; retained process id=$($p.Id)" }
Write-Output "$Name exit=$($p.ExitCode)"
Get-Content -LiteralPath (Join-Path $qa ($Name+'.out.log'))
Get-Content -LiteralPath (Join-Path $qa ($Name+'.err.log'))
if($p.ExitCode -ne 0) { throw "$Name native process failed, exit=$($p.ExitCode)" }
if($Capture -and (-not (Test-Path -LiteralPath $png) -or (Get-Item -LiteralPath $png).LastWriteTimeUtc -lt $started.AddSeconds(-1))) { throw 'No fresh restored-window screenshot' }
