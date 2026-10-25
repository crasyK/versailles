<#
.SYNOPSIS
  Read-only checks for the Versailles clean-machine run (docs/RELEASE-CHECKLIST-1.0.md, #52).

.DESCRIPTION
  Looks at what can be checked automatically on the machine under test and prints
  one line per check: PASS, FAIL, WARN or INFO. It changes nothing: no files, no
  registry values, no processes, no settings. It reads the install, the running
  processes, the Windows taskbar, the RunOnce / Run registry values, the Widgets
  folder, the SDK versions, the logs and the WebView2 runtime, and sends one GET
  to the local file server. (Add-Type compiles a small read-only helper for the
  taskbar and monitor checks.)

  Needs Windows PowerShell 5.1 (part of Windows); no Node, no repo.

  Run it from a normal (not elevated) PowerShell as the user under test:
    powershell -NoProfile -ExecutionPolicy Bypass -File clean-machine-check.ps1 -Expect 1.0.0
  After uninstalling:
    powershell -NoProfile -ExecutionPolicy Bypass -File clean-machine-check.ps1 -Phase uninstalled

  The exit code is the number of FAIL lines.

.PARAMETER Expect
  The version that should be installed (for example 1.0.0). Without it the
  installed version is only reported.

.PARAMETER Phase
  installed (default): Versailles is installed and should be running.
  uninstalled: Versailles was just uninstalled; checks that nothing was left behind
  and that the Widgets folder was kept.

.PARAMETER Root
  The Widgets folder. Default: VERSAILLES_ROOT if set, else Documents\Widgets.

.PARAMETER LogHours
  When Versailles is not running, how many hours of the log to scan. Default 2.
#>
[CmdletBinding()]
param(
  [string]$Expect = '',
  [ValidateSet('installed', 'uninstalled')]
  [string]$Phase = 'installed',
  [string]$Root = '',
  [int]$LogHours = 2
)

$ErrorActionPreference = 'Stop'
Set-StrictMode -Version 2.0

$script:Fails = 0
$script:Warns = 0
$script:Passes = 0

function Write-Check([string]$Kind, [string]$Name, [string]$Detail) {
  switch ($Kind) {
    'PASS' { $script:Passes++ }
    'FAIL' { $script:Fails++ }
    'WARN' { $script:Warns++ }
  }
  Write-Output ('{0,-5} {1,-24} {2}' -f $Kind, $Name, $Detail)
}
function Pass([string]$n, [string]$d) { Write-Check 'PASS' $n $d }
function Fail([string]$n, [string]$d) { Write-Check 'FAIL' $n $d }
function Warn([string]$n, [string]$d) { Write-Check 'WARN' $n $d }
function Info([string]$n, [string]$d) { Write-Check 'INFO' $n $d }

function Get-Prop($Object, [string]$Name) {
  if ($null -eq $Object) { return $null }
  $p = $Object.PSObject.Properties[$Name]
  if ($null -eq $p) { return $null }
  return $p.Value
}

function Get-RegValue([string]$Path, [string]$Name) {
  try {
    $item = Get-ItemProperty -LiteralPath $Path -ErrorAction Stop
    return Get-Prop $item $Name
  } catch {
    return $null
  }
}

function Short-Hash([string]$Path) {
  try {
    return (Get-FileHash -LiteralPath $Path -Algorithm SHA256).Hash.Substring(0, 16).ToLowerInvariant()
  } catch {
    return '?'
  }
}

# Read a file another process is writing (the log) without locking it.
function Read-Shared([string]$Path) {
  $fs = [IO.File]::Open($Path, [IO.FileMode]::Open, [IO.FileAccess]::Read, [IO.FileShare]'ReadWrite, Delete')
  try {
    $sr = New-Object IO.StreamReader($fs, [Text.Encoding]::UTF8)
    return $sr.ReadToEnd()
  } finally {
    $fs.Dispose()
  }
}

function Parse-Version([string]$Text) {
  if (-not $Text) { return $null }
  $m = [regex]::Match($Text, '(\d+)\.(\d+)\.(\d+)')
  if (-not $m.Success) { return $null }
  return [version]('{0}.{1}.{2}' -f $m.Groups[1].Value, $m.Groups[2].Value, $m.Groups[3].Value)
}

# --- native helper (read-only calls only) ---------------------------------
$helper = @'
using System;
using System.Collections.Generic;
using System.Runtime.InteropServices;

public static class VersaillesCheck {
  [DllImport("user32.dll", CharSet = CharSet.Unicode)]
  static extern IntPtr FindWindowEx(IntPtr parent, IntPtr after, string cls, string title);
  [DllImport("user32.dll")]
  static extern bool IsWindowVisible(IntPtr h);
  [DllImport("user32.dll")]
  static extern IntPtr SetThreadDpiAwarenessContext(IntPtr ctx);

  [StructLayout(LayoutKind.Sequential)]
  public struct RECT { public int Left, Top, Right, Bottom; }

  [StructLayout(LayoutKind.Sequential, CharSet = CharSet.Unicode)]
  struct MONITORINFOEX {
    public int cbSize; public RECT rcMonitor; public RECT rcWork; public uint dwFlags;
    [MarshalAs(UnmanagedType.ByValTStr, SizeConst = 32)] public string szDevice;
  }
  delegate bool MonitorEnumProc(IntPtr hMon, IntPtr hdc, IntPtr rect, IntPtr data);
  [DllImport("user32.dll")]
  static extern bool EnumDisplayMonitors(IntPtr hdc, IntPtr clip, MonitorEnumProc cb, IntPtr data);
  [DllImport("user32.dll", CharSet = CharSet.Unicode)]
  static extern bool GetMonitorInfo(IntPtr h, ref MONITORINFOEX mi);
  [DllImport("shcore.dll")]
  static extern int GetDpiForMonitor(IntPtr h, int type, out uint x, out uint y);

  [StructLayout(LayoutKind.Sequential)]
  struct APPBARDATA { public int cbSize; public IntPtr hWnd; public uint uCallbackMessage; public uint uEdge; public RECT rc; public IntPtr lParam; }
  [DllImport("shell32.dll")]
  static extern UIntPtr SHAppBarMessage(uint msg, ref APPBARDATA data);

  // Number of taskbar windows of a class, and how many of them are visible.
  public static int[] Taskbars(string cls) {
    int all = 0, visible = 0;
    IntPtr h = IntPtr.Zero;
    while ((h = FindWindowEx(IntPtr.Zero, h, cls, null)) != IntPtr.Zero) {
      all++;
      if (IsWindowVisible(h)) visible++;
    }
    return new int[] { all, visible };
  }

  // ABM_GETSTATE: bit 1 = auto-hide.
  public static bool AutoHide() {
    APPBARDATA d = new APPBARDATA();
    d.cbSize = Marshal.SizeOf(typeof(APPBARDATA));
    return ((ulong)SHAppBarMessage(4, ref d) & 1UL) != 0;
  }

  public static string[] Monitors() {
    List<string> list = new List<string>();
    // Per-monitor aware on this thread only, so sizes and scales are real.
    IntPtr old = IntPtr.Zero;
    try { old = SetThreadDpiAwarenessContext(new IntPtr(-4)); } catch { }
    try {
      EnumDisplayMonitors(IntPtr.Zero, IntPtr.Zero, delegate (IntPtr hMon, IntPtr hdc, IntPtr r, IntPtr d) {
        MONITORINFOEX mi = new MONITORINFOEX();
        mi.cbSize = Marshal.SizeOf(typeof(MONITORINFOEX));
        GetMonitorInfo(hMon, ref mi);
        uint dx = 96, dy = 96;
        try { GetDpiForMonitor(hMon, 0, out dx, out dy); } catch { }
        list.Add(string.Format("{0} {1}x{2} at {3},{4} scale {5}%{6}",
          mi.szDevice, mi.rcMonitor.Right - mi.rcMonitor.Left, mi.rcMonitor.Bottom - mi.rcMonitor.Top,
          mi.rcMonitor.Left, mi.rcMonitor.Top, dx * 100 / 96, (mi.dwFlags & 1) != 0 ? " primary" : ""));
        return true;
      }, IntPtr.Zero);
    } finally {
      if (old != IntPtr.Zero) { try { SetThreadDpiAwarenessContext(old); } catch { } }
    }
    return list.ToArray();
  }
}
'@
$native = $true
try {
  if (-not ('VersaillesCheck' -as [type])) { Add-Type -TypeDefinition $helper -Language CSharp }
} catch {
  $native = $false
}

$appData = Join-Path $env:LOCALAPPDATA 'com.versailles.widgets'
$logDir = Join-Path $appData 'logs'
$runOnceKey = 'HKCU:\Software\Microsoft\Windows\CurrentVersion\RunOnce'
$runKey = 'HKCU:\Software\Microsoft\Windows\CurrentVersion\Run'

Write-Output ''
Write-Output ('Versailles clean-machine check ({0}), {1}' -f $Phase, (Get-Date -Format 'yyyy-MM-dd HH:mm:ss'))
Write-Output ''

# --- system ----------------------------------------------------------------
$cv = Get-ItemProperty 'HKLM:\SOFTWARE\Microsoft\Windows NT\CurrentVersion'
$build = [int](Get-Prop $cv 'CurrentBuild')
$edition = Get-Prop $cv 'EditionID'
$osText = 'build {0}.{1} {2} {3}' -f $build, (Get-Prop $cv 'UBR'), (Get-Prop $cv 'DisplayVersion'), $edition
if ($build -ge 22000) { Pass 'windows 11' $osText } else { Fail 'windows 11' ($osText + ' (not Windows 11)') }
Info 'display language' ('{0} (format {1})' -f (Get-UICulture).Name, (Get-Culture).Name)
Info 'user' ('{0} (elevated: {1})' -f $env:USERNAME, ([Security.Principal.WindowsPrincipal][Security.Principal.WindowsIdentity]::GetCurrent()).IsInRole([Security.Principal.WindowsBuiltInRole]::Administrator))
if ($native) {
  $mons = [VersaillesCheck]::Monitors()
  Info 'monitors' ('{0}' -f $mons.Count)
  foreach ($m in $mons) { Info '  monitor' $m }
} else {
  Warn 'native helper' 'Add-Type failed: taskbar and monitor checks skipped'
}
$sac = Get-RegValue 'HKLM:\SYSTEM\CurrentControlSet\Control\CI\Policy' 'VerifiedAndReputablePolicyState'
if ($null -ne $sac) {
  $sacText = switch ($sac) { 0 { 'off' } 1 { 'on (blocks unsigned apps)' } 2 { 'evaluation' } default { [string]$sac } }
  Info 'smart app control' $sacText
}

# --- WebView2 --------------------------------------------------------------
$wvGuid = '{F3017226-FE2A-4295-8BDF-00C3A9A7E4C5}'
$wv = $null
foreach ($k in @("HKLM:\SOFTWARE\WOW6432Node\Microsoft\EdgeUpdate\Clients\$wvGuid",
                 "HKLM:\SOFTWARE\Microsoft\EdgeUpdate\Clients\$wvGuid",
                 "HKCU:\Software\Microsoft\EdgeUpdate\Clients\$wvGuid")) {
  $pv = Get-RegValue $k 'pv'
  if ($pv -and $pv -ne '0.0.0.0') { $wv = $pv; break }
}
if ($wv) { Pass 'webview2 runtime' $wv } else { Fail 'webview2 runtime' 'not found in EdgeUpdate\Clients' }

# --- install ---------------------------------------------------------------
$uninstallKeys = @('HKCU:\Software\Microsoft\Windows\CurrentVersion\Uninstall',
                   'HKLM:\SOFTWARE\Microsoft\Windows\CurrentVersion\Uninstall',
                   'HKLM:\SOFTWARE\WOW6432Node\Microsoft\Windows\CurrentVersion\Uninstall')
$entry = $null
foreach ($k in $uninstallKeys) {
  try {
    foreach ($sub in (Get-ChildItem -LiteralPath $k -ErrorAction Stop)) {
      $p = Get-ItemProperty -LiteralPath $sub.PSPath
      if ((Get-Prop $p 'DisplayName') -eq 'Versailles') { $entry = $p; break }
    }
  } catch { }
  if ($entry) { break }
}
$installDir = Join-Path $env:LOCALAPPDATA 'Versailles'
if ($entry) {
  $loc = [string](Get-Prop $entry 'InstallLocation')
  if ($loc) { $installDir = $loc.Trim('"') }
}
$exe = Join-Path $installDir 'versailles.exe'
$exeThere = Test-Path -LiteralPath $exe

if ($Phase -eq 'installed') {
  if ($entry) {
    $dv = [string](Get-Prop $entry 'DisplayVersion')
    $detail = 'version {0} in {1} ({2})' -f $dv, $installDir, ($(if ($entry.PSPath -like '*HKEY_CURRENT_USER*') { 'per user' } else { 'per machine' }))
    if ($Expect -and (Parse-Version $dv) -ne (Parse-Version $Expect)) { Fail 'installed' ($detail + ', expected ' + $Expect) } else { Pass 'installed' $detail }
  } else {
    Fail 'installed' 'no "Versailles" entry under ...\CurrentVersion\Uninstall'
  }
  if ($exeThere) {
    $fv = (Get-Item -LiteralPath $exe).VersionInfo.ProductVersion
    if ($Expect -and (Parse-Version $fv) -ne (Parse-Version $Expect)) { Fail 'versailles.exe' ('{0} is {1}, expected {2}' -f $exe, $fv, $Expect) } else { Pass 'versailles.exe' ('{0} ({1})' -f $exe, $fv) }
    $sig = Get-AuthenticodeSignature -LiteralPath $exe
    Info 'signature' ('{0} (releases are unsigned by design)' -f $sig.Status)
  } else {
    Fail 'versailles.exe' ('missing: ' + $exe)
  }
} else {
  if ($entry) { Fail 'uninstalled' 'the "Versailles" uninstall entry is still there' } else { Pass 'uninstalled' 'no uninstall entry' }
  if ($exeThere) { Fail 'versailles.exe' ('still there: ' + $exe) } else { Pass 'versailles.exe' 'removed' }
}

# --- processes -------------------------------------------------------------
$all = @(Get-CimInstance Win32_Process)
$byId = @{}
foreach ($p in $all) { $byId[[int]$p.ProcessId] = $p }
$vs = @($all | Where-Object { $_.Name -ieq 'versailles.exe' })
$roles = @()
foreach ($p in $vs) {
  $cmd = [string]$p.CommandLine
  $parent = $byId[[int]$p.ParentProcessId]
  $role = 'app'
  if ($cmd -match '--taskbar-guard') { $role = 'taskbar guard' }
  elseif ($cmd -match '--keys-hook') { $role = 'key helper' }
  elseif ($cmd -match '--restore-taskbar|--diagnostics') { $role = 'one-shot' }
  elseif ($null -eq $parent -or $parent.Name -ine 'versailles.exe') { $role = 'supervisor' }
  $roles += [pscustomobject]@{ Role = $role; Pid = [int]$p.ProcessId; Path = [string]$p.ExecutablePath; Started = $p.CreationDate; Cmd = $cmd }
}
$main = @($roles | Where-Object { $_.Role -eq 'supervisor' -or $_.Role -eq 'app' })
$running = $main.Count -gt 0
$sups = @($roles | Where-Object { $_.Role -eq 'supervisor' })
$startedAt = $null
if ($running) { $startedAt = ($main | Sort-Object Started | Select-Object -First 1).Started }

if ($Phase -eq 'installed') {
  if ($running) {
    Pass 'running' ('{0} Versailles process(es), started {1:HH:mm:ss}' -f $vs.Count, $startedAt)
    if ($sups.Count -gt 1) { Fail 'one instance' ('{0} supervisors: two copies of Versailles are running' -f $sups.Count) }
    foreach ($r in $roles) { Info ('  ' + $r.Role) ('pid {0} {1}' -f $r.Pid, $r.Cmd) }
    $foreign = @($main | Where-Object { $_.Path -and $exeThere -and ($_.Path -ine (Resolve-Path -LiteralPath $exe).Path) })
    if ($foreign.Count -gt 0) { Warn 'running build' ('not the installed exe: ' + $foreign[0].Path) }
    # WebView2 processes below this instance.
    $ids = @{}
    foreach ($r in $roles) { $ids[$r.Pid] = $true }
    $wvCount = 0
    $changed = $true
    while ($changed) {
      $changed = $false
      foreach ($p in $all) {
        $id = [int]$p.ProcessId
        if (-not $ids.ContainsKey($id) -and $ids.ContainsKey([int]$p.ParentProcessId)) {
          $ids[$id] = $true
          $changed = $true
          if ($p.Name -ieq 'msedgewebview2.exe') { $wvCount++ }
        }
      }
    }
    Info 'webview2 processes' ([string]$wvCount)
  } else {
    Fail 'running' 'no Versailles process (start it from Start, or check the logs below)'
  }
} else {
  if ($vs.Count -gt 0) { Fail 'processes' ('{0} versailles.exe still running' -f $vs.Count) } else { Pass 'processes' 'none running' }
}

# --- Widgets folder and page config -----------------------------------------
if (-not $Root) {
  if ($env:VERSAILLES_ROOT) { $Root = $env:VERSAILLES_ROOT } else { $Root = Join-Path ([Environment]::GetFolderPath('MyDocuments')) 'Widgets' }
}
$page = Join-Path $Root 'desktop\index.html'
$cfg = $null
$hideTaskbar = $false
$autostart = $true
$apiPort = 47831

if ($Phase -eq 'uninstalled') {
  if (Test-Path -LiteralPath $page) { Pass 'widgets kept' ('{0} (index.html {1})' -f $Root, (Short-Hash $page)) }
  elseif (Test-Path -LiteralPath $Root) { Pass 'widgets kept' ($Root + ' (no desktop\index.html)') }
  else { Fail 'widgets kept' ('missing: ' + $Root) }
} else {
  if (Test-Path -LiteralPath $page) {
    Pass 'page' ('{0} (sha256 {1})' -f $page, (Short-Hash $page))
    $html = [IO.File]::ReadAllText($page, [Text.Encoding]::UTF8)
    $m = [regex]::Match($html, '(?is)<script[^>]*\bid\s*=\s*["'']versailles["''][^>]*>(.*?)</script>')
    if ($m.Success) {
      try {
        $cfg = $m.Groups[1].Value | ConvertFrom-Json
        Pass 'page config' '#versailles parses as JSON'
      } catch {
        Fail 'page config' ('#versailles is not valid JSON: ' + $_.Exception.Message)
      }
    } else {
      $side = Join-Path $Root 'versailles.json'
      if (Test-Path -LiteralPath $side) {
        try { $cfg = Get-Content -LiteralPath $side -Raw | ConvertFrom-Json; Info 'page config' 'no #versailles block; using versailles.json' }
        catch { Fail 'page config' ('versailles.json is not valid JSON: ' + $_.Exception.Message) }
      } else {
        Info 'page config' 'no #versailles block: defaults'
      }
    }
    if ($cfg) {
      $hideTaskbar = (Get-Prop (Get-Prop $cfg 'shell') 'hideTaskbar') -eq $true
      if ((Get-Prop $cfg 'autostart') -eq $false) { $autostart = $false }
      $port = Get-Prop (Get-Prop $cfg 'api') 'port'
      if ($port) { $apiPort = [int]$port }
      $upd = Get-Prop (Get-Prop $cfg 'updates') 'check'
      Info 'config' ('hideTaskbar {0}, autostart {1}, updates.check {2}, suppress [{3}]' -f $hideTaskbar, $autostart, ($upd -ne $false), ((@(Get-Prop (Get-Prop $cfg 'shell') 'suppress')) -join ', '))
    }
  } else {
    Fail 'page' ('missing: ' + $page + ' (first run not done, or "I''ll bring my own")')
  }
  $bar = Join-Path $Root 'desktop\bar.js'
  if (Test-Path -LiteralPath $bar) { Info 'bar.js' ('sha256 ' + (Short-Hash $bar)) } else { Info 'bar.js' 'not there (no starter command bar)' }
  foreach ($rel in @('.sdk\versailles.d.ts', '.sdk\xterm.js')) {
    if (Test-Path -LiteralPath (Join-Path $Root $rel)) { Info $rel 'present' } else { Info $rel 'missing' }
  }
  $rt = Join-Path $Root '.versailles\config.json'
  if (Test-Path -LiteralPath $rt) {
    try {
      $rc = Get-Content -LiteralPath $rt -Raw | ConvertFrom-Json
      if (Get-Prop $rc 'apiToken') { Pass 'runtime config' '.versailles\config.json parses, token set (not shown)' } else { Warn 'runtime config' '.versailles\config.json has no apiToken' }
      $bound = Get-Prop $rc 'apiBoundPort'
      if ($bound) { $apiPort = [int]$bound }
    } catch {
      Fail 'runtime config' ('.versailles\config.json is not valid JSON: ' + $_.Exception.Message)
    }
  } elseif ($running) {
    Fail 'runtime config' '.versailles\config.json missing while Versailles runs'
  }
  $store = Join-Path $Root '.versailles\storage'
  if (Test-Path -LiteralPath $store) { Info 'storage' ('{0} store file(s)' -f @(Get-ChildItem -LiteralPath $store -Filter *.json).Count) }
  $marker = Join-Path $appData 'first-run-offered'
  if (Test-Path -LiteralPath $marker) { Info 'first run' ('answered ' + (Get-Item -LiteralPath $marker).LastWriteTime.ToString('yyyy-MM-dd HH:mm')) } else { Info 'first run' 'not answered yet' }
}

# --- SDK ---------------------------------------------------------------------
if ($Phase -eq 'installed') {
  $sdkPath = Join-Path $Root '.sdk\versailles.js'
  $desk = $null
  if (Test-Path -LiteralPath $sdkPath) {
    $first = (Get-Content -LiteralPath $sdkPath -TotalCount 1)
    $mm = [regex]::Match([string]$first, 'versailles-sdk\s+([0-9][0-9A-Za-z.+\-]*)')
    if ($mm.Success) { $desk = $mm.Groups[1].Value }
  }
  $bundled = @()
  if ($exeThere) {
    try {
      $bytes = [IO.File]::ReadAllBytes($exe)
      $text = [Text.Encoding]::GetEncoding(28591).GetString($bytes)
      $bundled = @([regex]::Matches($text, '/\*! versailles-sdk ([0-9][0-9A-Za-z.+\-]*) \*/') | ForEach-Object { $_.Groups[1].Value } | Select-Object -Unique)
    } catch { }
  }
  $bak = Test-Path -LiteralPath (Join-Path $Root '.sdk\versailles.js.bak')
  if ($bundled.Count -eq 0) {
    Warn 'sdk' ('desktop SDK {0}; could not read the SDK inside versailles.exe' -f $desk)
  } elseif (-not $desk) {
    Warn 'sdk' ('no desktop SDK (.sdk\versailles.js); the app serves its own {0}' -f ($bundled -join ', '))
  } elseif ($bundled -contains $desk) {
    Pass 'sdk' ('desktop {0} = bundled{1}' -f $desk, ($(if ($bak) { '; versailles.js.bak kept from the update' } else { '' })))
  } else {
    Fail 'sdk' ('desktop {0}, bundled {1}: edited by hand (then left alone on purpose) or not updated; see "sdk" in Versailles.log' -f $desk, ($bundled -join ', '))
  }
  # The file server answers (one GET on localhost).
  if ($running) {
    try {
      $r = Invoke-WebRequest -UseBasicParsing -TimeoutSec 5 -Uri ('http://127.0.0.1:{0}/files/desktop/index.html' -f $apiPort)
      Pass 'file server' ('http://127.0.0.1:{0}/files/ answers {1}' -f $apiPort, $r.StatusCode)
    } catch {
      Fail 'file server' ('http://127.0.0.1:{0}/files/desktop/index.html: {1}' -f $apiPort, $_.Exception.Message)
    }
  }
}

# --- taskbar, RunOnce, autostart ---------------------------------------------
$restoreFile = Join-Path $appData 'taskbar-restore.json'
$restoreThere = Test-Path -LiteralPath $restoreFile
$expectHidden = ($Phase -eq 'installed') -and $running -and $hideTaskbar
if ($native) {
  $primary = [VersaillesCheck]::Taskbars('Shell_TrayWnd')
  $second = [VersaillesCheck]::Taskbars('Shell_SecondaryTrayWnd')
  $autoHide = [VersaillesCheck]::AutoHide()
  $state = 'primary {0}, secondary {1} of {2} visible, auto-hide {3}' -f ($(if ($primary[1] -gt 0) { 'visible' } elseif ($primary[0] -gt 0) { 'hidden' } else { 'missing (Explorer not running?)' })), $second[1], $second[0], $autoHide
  if ($primary[0] -eq 0) {
    Fail 'taskbar' $state
  } elseif ($expectHidden) {
    if ($primary[1] -eq 0 -and $second[1] -eq 0) { Pass 'taskbar hidden' $state } else { Fail 'taskbar hidden' ('hideTaskbar is on but: ' + $state) }
  } else {
    if ($primary[1] -gt 0 -and $second[1] -eq $second[0]) { Pass 'taskbar visible' $state } else { Fail 'taskbar visible' ('should be back: ' + $state + ' (Task Manager > Windows Explorer > Restart)') }
  }
  if (-not $expectHidden -and $autoHide) {
    if ($restoreThere) { Fail 'auto-hide' 'auto-hide is on and taskbar-restore.json is still there: Versailles did not put your setting back' }
    else { Warn 'auto-hide' 'auto-hide is on (your own setting? Versailles restores what it saved)' }
  }
}
if ($expectHidden) {
  if ($restoreThere) { Pass 'restore file' ('saved state: ' + (Get-Content -LiteralPath $restoreFile -Raw).Trim()) } else { Fail 'restore file' ('missing while hidden: ' + $restoreFile) }
} elseif ($restoreThere) {
  Fail 'restore file' ('left behind while the taskbar is not hidden: ' + $restoreFile)
} else {
  Pass 'restore file' 'none (taskbar not hidden)'
}

$runOnce = Get-RegValue $runOnceKey 'VersaillesRestoreTaskbar'
if ($expectHidden) {
  if ($runOnce) { Pass 'runonce' ('set while hidden: ' + $runOnce) } else { Fail 'runonce' 'VersaillesRestoreTaskbar missing while the taskbar is hidden (no logon safety net)' }
} elseif ($runOnce) {
  Fail 'runonce' ('left behind: VersaillesRestoreTaskbar = ' + $runOnce)
} else {
  Pass 'runonce' 'no VersaillesRestoreTaskbar value'
}
if ($running) {
  $guards = @($roles | Where-Object { $_.Role -eq 'taskbar guard' })
  if ($expectHidden -and $guards.Count -eq 0) { Fail 'taskbar guard' 'no guard process while the taskbar is hidden (a crash would leave it hidden until logon)' }
  elseif ($expectHidden) { Pass 'taskbar guard' ('pid ' + $guards[0].Pid) }
}

$runEntries = @()
try {
  $runProps = Get-ItemProperty -LiteralPath $runKey -ErrorAction Stop
  foreach ($pp in $runProps.PSObject.Properties) {
    if ($pp.Name -notlike 'PS*' -and ([string]$pp.Value) -match 'versailles\.exe') { $runEntries += ('{0} = {1}' -f $pp.Name, $pp.Value) }
  }
} catch { }
if ($Phase -eq 'uninstalled') {
  if ($runEntries.Count -gt 0) { Fail 'autostart' ('Run entry left behind: ' + ($runEntries -join '; ')) } else { Pass 'autostart' 'no Run entry' }
} elseif ($autostart) {
  if ($runEntries.Count -gt 0) { Pass 'autostart' ($runEntries -join '; ') } else { Fail 'autostart' 'autostart is on but there is no Run entry for versailles.exe' }
} else {
  if ($runEntries.Count -gt 0) { Fail 'autostart' ('autostart is off but: ' + ($runEntries -join '; ')) } else { Pass 'autostart' 'off, no Run entry' }
}

# --- logs --------------------------------------------------------------------
if ($Phase -eq 'installed') {
  $since = $startedAt
  if (-not $since) { $since = (Get-Date).AddHours(-$LogHours) }
  $log = Join-Path $logDir 'Versailles.log'
  if (Test-Path -LiteralPath $log) {
    $errors = @()
    $warnCount = 0
    $noise = 0
    $lineRe = '^\[(\d{4}-\d{2}-\d{2})\]\[(\d{2}:\d{2}:\d{2})\]\[([^\]]*)\]\[([A-Z]+)\]\s?(.*)$'
    foreach ($line in ((Read-Shared $log) -split "`r?`n")) {
      $mm = [regex]::Match($line, $lineRe)
      if (-not $mm.Success) { continue }
      # Versailles.log stamps are local time from 0.12.0 on (UTC before).
      $when = [datetime]::ParseExact($mm.Groups[1].Value + ' ' + $mm.Groups[2].Value, 'yyyy-MM-dd HH:mm:ss', [Globalization.CultureInfo]::InvariantCulture)
      if ($when -lt $since.AddSeconds(-1)) { continue }
      if ($mm.Groups[4].Value -eq 'ERROR') {
        # Known noise, not a failure: a host window asking for a favicon it does not have.
        if ($mm.Groups[5].Value -match '^asset not found: favicon\.ico') { $noise++; continue }
        $errors += ('{0:HH:mm:ss} {1}' -f $when, $mm.Groups[5].Value)
      }
      elseif ($mm.Groups[4].Value -eq 'WARN') { $warnCount++ }
    }
    $range = 'since {0:yyyy-MM-dd HH:mm:ss}' -f $since
    if ($noise -gt 0) { Info 'log noise' ('{0} "asset not found: favicon.ico" line(s), ignored' -f $noise) }
    if ($errors.Count -eq 0) { Pass 'log errors' ('none ' + $range + ('; {0} warning(s)' -f $warnCount)) }
    else {
      Fail 'log errors' ('{0} ERROR line(s) {1}; {2} warning(s)' -f $errors.Count, $range, $warnCount)
      foreach ($e in ($errors | Select-Object -First 8)) { Info '  error' $e }
    }
  } else {
    Warn 'log' ('no log yet: ' + $log)
  }
  $crash = Join-Path $logDir 'last-crash.txt'
  if ((Test-Path -LiteralPath $crash) -and (Get-Item -LiteralPath $crash).LastWriteTime -ge $since) {
    Fail 'crash' ('last-crash.txt written {0:HH:mm:ss}: {1}' -f (Get-Item -LiteralPath $crash).LastWriteTime, ((Get-Content -LiteralPath $crash -TotalCount 1) -join ''))
  } else {
    Pass 'crash' 'no crash recorded since start'
  }
  $sup = Join-Path $logDir 'supervisor.log'
  if (Test-Path -LiteralPath $sup) {
    $restarts = @(Get-Content -LiteralPath $sup | Where-Object {
      $mm = [regex]::Match($_, '^\[(\d{4}-\d{2}-\d{2} \d{2}:\d{2}:\d{2})\]')
      $mm.Success -and ([datetime]::ParseExact($mm.Groups[1].Value, 'yyyy-MM-dd HH:mm:ss', [Globalization.CultureInfo]::InvariantCulture) -ge $since)
    })
    if ($restarts.Count -gt 0) { Warn 'supervisor' ('{0} line(s) since start; last: {1}' -f $restarts.Count, $restarts[-1]) }
  }
  $tlog = Join-Path $logDir 'taskbar.log'
  if (Test-Path -LiteralPath $tlog) {
    $last = Get-Content -LiteralPath $tlog -Tail 1
    if ($last) { Info 'taskbar.log' $last }
  }
}

Write-Output ''
Write-Output ('{0} PASS, {1} FAIL, {2} WARN' -f $script:Passes, $script:Fails, $script:Warns)
exit $script:Fails
