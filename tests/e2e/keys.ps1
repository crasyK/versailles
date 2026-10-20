# e2e: press a key chord (virtual-key codes, pressed in order, released in
# reverse), then print the foreground window's process name and whether it is
# shell UI on screen (Start, Search; closed ones stay foreground but cloaked).
# If it opened anyway, close it with Esc so the desktop is left as it was.
param([string]$Chord)
Add-Type -Namespace E2E -Name K -MemberDefinition @'
[DllImport("user32.dll")] public static extern void keybd_event(byte vk, byte scan, uint flags, UIntPtr extra);
[DllImport("user32.dll")] public static extern IntPtr GetForegroundWindow();
[DllImport("user32.dll")] public static extern uint GetWindowThreadProcessId(IntPtr h, out uint pid);
[DllImport("dwmapi.dll")] public static extern int DwmGetWindowAttribute(IntPtr h, int a, out int v, int s);
'@
$keys = $Chord -split ',' | ForEach-Object { [byte][Convert]::ToInt32($_.Trim(), 16) }
foreach ($k in $keys) { [E2E.K]::keybd_event($k, 0, 0, [UIntPtr]::Zero); Start-Sleep -Milliseconds 30 }
[array]::Reverse($keys)
foreach ($k in $keys) { [E2E.K]::keybd_event($k, 0, 2, [UIntPtr]::Zero); Start-Sleep -Milliseconds 30 }
Start-Sleep -Milliseconds 800
$fg = [E2E.K]::GetForegroundWindow()
$fgPid = 0
[void][E2E.K]::GetWindowThreadProcessId($fg, [ref]$fgPid)
$name = (Get-Process -Id $fgPid -ErrorAction SilentlyContinue).ProcessName
$cloaked = 0
[void][E2E.K]::DwmGetWindowAttribute($fg, 14, [ref]$cloaked, 4)  # DWMWA_CLOAKED
$shellOpen = ($name -match 'StartMenuExperienceHost|SearchHost|SearchApp|ShellExperienceHost|ShellHost') -and $cloaked -eq 0
if ($shellOpen) {
  [E2E.K]::keybd_event(0x1B, 0, 0, [UIntPtr]::Zero); [E2E.K]::keybd_event(0x1B, 0, 2, [UIntPtr]::Zero)
}
@{ foreground = $name; shellOpen = $shellOpen } | ConvertTo-Json -Compress
