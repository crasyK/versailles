# e2e probe: visible texts of a process's top-level windows (UI Automation), and
# each monitor's reserved top / bottom strips, whether Explorer's taskbar is
# visible and whether the taskbar RunOnce restore is set. Prints one JSON object.
param([int]$ProcessId)
Add-Type -AssemblyName UIAutomationClient, UIAutomationTypes
Add-Type -Namespace E2E -Name U -MemberDefinition @'
[DllImport("user32.dll")] public static extern bool SetProcessDPIAware();
[DllImport("user32.dll", CharSet = CharSet.Unicode)] public static extern IntPtr FindWindow(string cls, string title);
[DllImport("user32.dll")] public static extern bool IsWindowVisible(IntPtr h);
'@
[E2E.U]::SetProcessDPIAware() | Out-Null
Add-Type -AssemblyName System.Windows.Forms

$texts = New-Object System.Collections.ArrayList
$root = [System.Windows.Automation.AutomationElement]::RootElement
$windows = $root.FindAll([System.Windows.Automation.TreeScope]::Children, [System.Windows.Automation.Condition]::TrueCondition)
foreach ($w in $windows) {
  if ($w.Current.ProcessId -ne $ProcessId) { continue }
  $all = $w.FindAll([System.Windows.Automation.TreeScope]::Descendants, [System.Windows.Automation.Condition]::TrueCondition)
  foreach ($e in $all) { if ($e.Current.Name) { [void]$texts.Add($e.Current.Name) } }
}
$monitors = [System.Windows.Forms.Screen]::AllScreens | ForEach-Object {
  @{ name = $_.DeviceName; top = $_.WorkingArea.Top - $_.Bounds.Top; bottom = $_.Bounds.Bottom - $_.WorkingArea.Bottom }
}
$tray = [E2E.U]::FindWindow('Shell_TrayWnd', $null)
$taskbar = ($tray -ne [IntPtr]::Zero) -and [E2E.U]::IsWindowVisible($tray)
$runOnce = $null -ne (Get-ItemProperty 'HKCU:\Software\Microsoft\Windows\CurrentVersion\RunOnce' -Name VersaillesRestoreTaskbar -ErrorAction SilentlyContinue)
@{ texts = @($texts); monitors = @($monitors); taskbar = $taskbar; runOnce = $runOnce } | ConvertTo-Json -Depth 4 -Compress
