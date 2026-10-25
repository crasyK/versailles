# e2e: one left click at the centre of the primary monitor (where the
# centred e2e overlay sits), then the cursor goes back where it was.
Add-Type -Namespace E2E -Name C -MemberDefinition @'
[DllImport("user32.dll")] public static extern bool SetProcessDPIAware();
[DllImport("user32.dll")] public static extern bool SetCursorPos(int x, int y);
[DllImport("user32.dll")] public static extern bool GetCursorPos(out POINT p);
[DllImport("user32.dll")] public static extern void mouse_event(uint flags, int dx, int dy, uint data, UIntPtr extra);
[StructLayout(LayoutKind.Sequential)] public struct POINT { public int X; public int Y; }
'@
[E2E.C]::SetProcessDPIAware() | Out-Null
Add-Type -AssemblyName System.Windows.Forms
$b = [System.Windows.Forms.Screen]::PrimaryScreen.Bounds
$was = New-Object E2E.C+POINT
[E2E.C]::GetCursorPos([ref]$was) | Out-Null
[E2E.C]::SetCursorPos($b.X + [int]($b.Width / 2), $b.Y + [int]($b.Height / 2)) | Out-Null
Start-Sleep -Milliseconds 150
[E2E.C]::mouse_event(0x0002, 0, 0, 0, [UIntPtr]::Zero)   # left down
Start-Sleep -Milliseconds 60
[E2E.C]::mouse_event(0x0004, 0, 0, 0, [UIntPtr]::Zero)   # left up
Start-Sleep -Milliseconds 150
[E2E.C]::SetCursorPos($was.X, $was.Y) | Out-Null
