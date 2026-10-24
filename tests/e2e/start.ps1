# e2e --take-taskbar: start a program (the user's Versailles, Explorer) so it
# outlives this test and the terminal: detached, and out of the terminal's job
# object when the job allows breakaway (a kill-on-close job would take it down
# with the terminal). Inherits this process's environment. Prints
# { pid, breakaway } as JSON.
param([string]$Exe, [string]$Arguments = '')
Add-Type -Namespace E2E -Name S -MemberDefinition @'
[StructLayout(LayoutKind.Sequential, CharSet = CharSet.Unicode)] public struct STARTUPINFO {
  public int cb; public string lpReserved; public string lpDesktop; public string lpTitle;
  public int dwX; public int dwY; public int dwXSize; public int dwYSize; public int dwXCountChars; public int dwYCountChars;
  public int dwFillAttribute; public int dwFlags; public short wShowWindow; public short cbReserved2;
  public IntPtr lpReserved2; public IntPtr hStdInput; public IntPtr hStdOutput; public IntPtr hStdError; }
[StructLayout(LayoutKind.Sequential)] public struct PROCESS_INFORMATION { public IntPtr hProcess; public IntPtr hThread; public int dwProcessId; public int dwThreadId; }
[DllImport("kernel32.dll", SetLastError = true, CharSet = CharSet.Unicode)] public static extern bool CreateProcess(string app, System.Text.StringBuilder cmd,
  IntPtr pa, IntPtr ta, bool inherit, uint flags, IntPtr env, string dir, ref STARTUPINFO si, out PROCESS_INFORMATION pi);
[DllImport("kernel32.dll")] public static extern bool CloseHandle(IntPtr h);
'@
$DETACHED_PROCESS = 0x8; $CREATE_NEW_PROCESS_GROUP = 0x200; $CREATE_BREAKAWAY_FROM_JOB = 0x01000000
$dir = Split-Path -Parent $Exe
foreach ($breakaway in @($true, $false)) {
  $flags = $DETACHED_PROCESS -bor $CREATE_NEW_PROCESS_GROUP
  if ($breakaway) { $flags = $flags -bor $CREATE_BREAKAWAY_FROM_JOB }
  $si = New-Object E2E.S+STARTUPINFO; $si.cb = [Runtime.InteropServices.Marshal]::SizeOf($si)
  $pi = New-Object E2E.S+PROCESS_INFORMATION
  $cmd = New-Object System.Text.StringBuilder ("`"$Exe`" $Arguments".Trim())
  if ([E2E.S]::CreateProcess($Exe, $cmd, [IntPtr]::Zero, [IntPtr]::Zero, $false, [uint32]$flags, [IntPtr]::Zero, $dir, [ref]$si, [ref]$pi)) {
    [void][E2E.S]::CloseHandle($pi.hThread); [void][E2E.S]::CloseHandle($pi.hProcess)
    @{ pid = $pi.dwProcessId; breakaway = $breakaway } | ConvertTo-Json -Compress
    exit 0
  }
  # Access denied: the job does not allow breakaway; start inside it.
}
@{ pid = 0; error = "CreateProcess failed: $([Runtime.InteropServices.Marshal]::GetLastWin32Error())" } | ConvertTo-Json -Compress
exit 1
