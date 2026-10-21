param([Parameter(Mandatory=$true)][string]$Executable,[Parameter(Mandatory=$true)][string]$OutputDirectory)
$ErrorActionPreference='Stop'
[Console]::OutputEncoding=[System.Text.UTF8Encoding]::new()
$projectRoot=Split-Path -Parent $PSScriptRoot
$outputFull=[System.IO.Path]::GetFullPath($OutputDirectory)
$allowedRoot=[System.IO.Path]::GetFullPath((Join-Path $projectRoot 'target\endurance'))+[System.IO.Path]::DirectorySeparatorChar
if(-not $outputFull.StartsWith($allowedRoot,[System.StringComparison]::OrdinalIgnoreCase)){throw 'Diagnostic output must be a new directory under target/endurance'}
if(Test-Path -LiteralPath $outputFull){throw 'Refusing to reuse or overwrite an existing diagnostic run directory'}
New-Item -ItemType Directory -Path $outputFull | Out-Null
Add-Type -TypeDefinition @'
using System;
using System.Text;
using System.Collections.Generic;
using System.Runtime.InteropServices;
public static class EnduranceMonitor {
 [StructLayout(LayoutKind.Sequential)] public struct RECT{public int Left,Top,Right,Bottom;}
 [DllImport("user32.dll")] public static extern uint GetWindowThreadProcessId(IntPtr h,out uint p);
 [DllImport("user32.dll")] public static extern bool IsIconic(IntPtr h);
 [DllImport("user32.dll")] public static extern bool IsWindowVisible(IntPtr h);
 [DllImport("user32.dll")] public static extern bool GetClientRect(IntPtr h,out RECT r);
 [DllImport("user32.dll")] public static extern bool ShowWindowAsync(IntPtr h,int c);
 [DllImport("user32.dll")] public static extern bool PostMessage(IntPtr h,uint msg,IntPtr w,IntPtr l);
 [DllImport("user32.dll")] public static extern uint GetDpiForWindow(IntPtr h);
 [DllImport("user32.dll")] public static extern int GetSystemMetrics(int n);
 [DllImport("kernel32.dll")] public static extern ulong GetTickCount64();
 [DllImport("kernel32.dll")] public static extern bool QueryUnbiasedInterruptTime(out ulong value);
 delegate bool EnumProc(IntPtr h,IntPtr l);
 [DllImport("user32.dll")] static extern bool EnumWindows(EnumProc callback,IntPtr l);
 [DllImport("user32.dll",CharSet=CharSet.Unicode)] static extern int GetWindowText(IntPtr h,StringBuilder b,int n);
 [DllImport("user32.dll",CharSet=CharSet.Unicode)] static extern int GetClassName(IntPtr h,StringBuilder b,int n);
 public static IntPtr Find(uint pid){IntPtr found=IntPtr.Zero;EnumWindows((h,l)=>{uint owner;GetWindowThreadProcessId(h,out owner);if(owner==pid){var t=new StringBuilder(512);var c=new StringBuilder(256);GetWindowText(h,t,512);GetClassName(h,c,256);if(c.ToString()!="ConsoleWindowClass"&&t.ToString().StartsWith("Audio Visualizer")){if(found!=IntPtr.Zero)throw new Exception("Ambiguous process window");found=h;}}return true;},IntPtr.Zero);return found;}
}
'@
$utcStart=[DateTime]::UtcNow
$clock=[Diagnostics.Stopwatch]::StartNew()
$stdout=Join-Path $outputFull 'app.out.log'
$stderr=Join-Path $outputFull 'app.err.log'
$process=Start-Process -FilePath $Executable -ArgumentList @('--demo','--qa-endurance',('"'+$outputFull+'"')) -WindowStyle Hidden -RedirectStandardOutput $stdout -RedirectStandardError $stderr -PassThru
$meta=[ordered]@{utc_start=$utcStart.ToString('o');pid=$process.Id;executable=(Resolve-Path -LiteralPath $Executable).Path;sha256=(Get-FileHash -LiteralPath $Executable -Algorithm SHA256).Hash;logical_processors=[Environment]::ProcessorCount;requested_seconds=600;no_intentional_minimize=$true;capture='only after measurement';hardware='synthetic only'}
$meta | ConvertTo-Json | Set-Content -LiteralPath (Join-Path $outputFull 'runner-start.json') -Encoding UTF8
$resources=[System.IO.StreamWriter]::new((Join-Path $outputFull 'resources.csv'),$false,[System.Text.UTF8Encoding]::new($false))
$resources.WriteLine('elapsed_s,utc_ms,cpu_s,working_bytes,private_bytes,virtual_bytes,handles,threads,minimized,visible,client_width,client_height,dpi,screen_width,screen_height,uptime_ms,awake_ms')
$window=[IntPtr]::Zero
$watchdog=$false
$kill=$false
try {
 while(-not $process.HasExited -and $clock.Elapsed.TotalSeconds -lt 630){
  $process.Refresh()
  if($window -eq [IntPtr]::Zero){
   $window=[EnduranceMonitor]::Find([uint32]$process.Id)
   if($window -ne [IntPtr]::Zero){
    [uint32]$owner=0;[void][EnduranceMonitor]::GetWindowThreadProcessId($window,[ref]$owner)
    if($owner -ne $process.Id){throw 'Window owner mismatch'}
    [void][EnduranceMonitor]::ShowWindowAsync($window,4)
    Write-Output "Owned render window shown without activation; elapsed=$($clock.Elapsed.TotalSeconds) pid=$owner"
   }
  }
  $rect=New-Object EnduranceMonitor+RECT
  [void][EnduranceMonitor]::GetClientRect($window,[ref]$rect)
  [uint64]$awake=0;[void][EnduranceMonitor]::QueryUnbiasedInterruptTime([ref]$awake)
  $values=@($clock.Elapsed.TotalSeconds,[DateTimeOffset]::UtcNow.ToUnixTimeMilliseconds(),$process.TotalProcessorTime.TotalSeconds,$process.WorkingSet64,$process.PrivateMemorySize64,$process.VirtualMemorySize64,$process.HandleCount,$process.Threads.Count,[int][EnduranceMonitor]::IsIconic($window),[int][EnduranceMonitor]::IsWindowVisible($window),($rect.Right-$rect.Left),($rect.Bottom-$rect.Top),[EnduranceMonitor]::GetDpiForWindow($window),[EnduranceMonitor]::GetSystemMetrics(0),[EnduranceMonitor]::GetSystemMetrics(1),[EnduranceMonitor]::GetTickCount64(),($awake/10000.0))
  $resources.WriteLine(($values | ForEach-Object {[Convert]::ToString($_,[Globalization.CultureInfo]::InvariantCulture)}) -join ',')
  $resources.Flush()
  if(([int]$clock.Elapsed.TotalSeconds)%30 -eq 0){Write-Output "Monitor $([math]::Round($clock.Elapsed.TotalSeconds,1)) s private=$($process.PrivateMemorySize64) working=$($process.WorkingSet64)"}
  Start-Sleep -Seconds 1
 }
 if(-not $process.HasExited){
  $watchdog=$true
  [uint32]$owner=0;[void][EnduranceMonitor]::GetWindowThreadProcessId($window,[ref]$owner)
  if($owner -eq $process.Id){[void][EnduranceMonitor]::PostMessage($window,0x10,[IntPtr]::Zero,[IntPtr]::Zero)}
  if(-not $process.WaitForExit(8000)){$kill=$true;$process.Kill();$process.WaitForExit()}
 }
}finally{$resources.Dispose()}
$process.Refresh()
$utcEnd=[DateTime]::UtcNow
$result=[ordered]@{utc_end=$utcEnd.ToString('o');elapsed_seconds=$clock.Elapsed.TotalSeconds;exit_code=$process.ExitCode;watchdog_close=$watchdog;watchdog_kill=$kill;summary_exists=(Test-Path -LiteralPath (Join-Path $outputFull 'summary.json'));screenshot_exists=(Test-Path -LiteralPath (Join-Path $outputFull 'final.png'))}
$result | ConvertTo-Json | Set-Content -LiteralPath (Join-Path $outputFull 'runner-result.json') -Encoding UTF8
# Read-only, bounded power-event query; never enable/change logging or power policy.
try {
 $events=Get-WinEvent -FilterHashtable @{LogName='System';ProviderName=@('Microsoft-Windows-Kernel-Power','Microsoft-Windows-Power-Troubleshooter');StartTime=$utcStart.AddSeconds(-5);EndTime=$utcEnd.AddSeconds(5)} -ErrorAction Stop
 $events | ForEach-Object {[ordered]@{utc=$_.TimeCreated.ToUniversalTime().ToString('o');provider=$_.ProviderName;id=$_.Id;message=$_.Message;xml=$_.ToXml()}} | ConvertTo-Json -Depth 4 | Set-Content -LiteralPath (Join-Path $outputFull 'power-events.json') -Encoding UTF8
}catch{ $_.ToString() | Set-Content -LiteralPath (Join-Path $outputFull 'power-events-query.txt') -Encoding UTF8 }
$result | ConvertTo-Json
Write-Output "Diagnostic output: $outputFull"
