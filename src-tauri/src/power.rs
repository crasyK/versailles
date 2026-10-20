//! `power` hook: lock, sleep, sign out, restart, shut down.

use crate::error::{AppError, AppResult};
use serde::Deserialize;

#[derive(Debug, Clone, Copy, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum PowerAction {
    Lock,
    Sleep,
    SignOut,
    Restart,
    Shutdown,
}

#[cfg(windows)]
pub fn run(action: PowerAction) -> AppResult<()> {
    use windows::Win32::System::Power::SetSuspendState;
    use windows::Win32::System::Shutdown::{
        ExitWindowsEx, LockWorkStation, EWX_LOGOFF, SHTDN_REASON_MAJOR_OTHER,
    };
    let err = |e: windows::core::Error| AppError::msg(e.to_string());
    match action {
        PowerAction::Lock => unsafe { LockWorkStation() }.map_err(err),
        PowerAction::Sleep => {
            // SetSuspendState needs SeShutdownPrivilege enabled; a normal
            // token holds it disabled.
            enable_shutdown_privilege()?;
            // Suspend (not hibernate), let apps veto, keep wake events.
            if unsafe { SetSuspendState(false, false, false) } {
                return Ok(());
            }
            let e = windows::core::Error::from_win32();
            // Modern Standby (S0 low power idle) PCs have no S3 sleep, and
            // Windows offers apps no supported way to enter Modern Standby.
            if e.code() == windows::Win32::Foundation::ERROR_NOT_SUPPORTED.to_hresult() {
                return Err(AppError::msg(
                    "sleep is not available: this PC only has Modern Standby, which apps cannot start (use the power button or Start menu)",
                ));
            }
            Err(AppError::msg(format!("sleep was refused: {e}")))
        }
        PowerAction::SignOut => unsafe { ExitWindowsEx(EWX_LOGOFF, SHTDN_REASON_MAJOR_OTHER) }.map_err(err),
        // shutdown.exe holds the shutdown privilege handling for us.
        PowerAction::Restart => shutdown_exe(&["/r", "/t", "0"]),
        PowerAction::Shutdown => shutdown_exe(&["/s", "/t", "0"]),
    }
}

/// Enable SeShutdownPrivilege on this process's token (held but disabled
/// for a normal user).
#[cfg(windows)]
fn enable_shutdown_privilege() -> AppResult<()> {
    use windows::Win32::Foundation::{CloseHandle, GetLastError, ERROR_NOT_ALL_ASSIGNED, HANDLE, LUID};
    use windows::Win32::Security::{
        AdjustTokenPrivileges, LookupPrivilegeValueW, LUID_AND_ATTRIBUTES, SE_PRIVILEGE_ENABLED,
        SE_SHUTDOWN_NAME, TOKEN_ADJUST_PRIVILEGES, TOKEN_PRIVILEGES, TOKEN_QUERY,
    };
    use windows::Win32::System::Threading::{GetCurrentProcess, OpenProcessToken};
    let err = |what: &str, e: windows::core::Error| AppError::msg(format!("sleep: {what}: {e}"));
    unsafe {
        let mut token = HANDLE::default();
        OpenProcessToken(GetCurrentProcess(), TOKEN_ADJUST_PRIVILEGES | TOKEN_QUERY, &mut token)
            .map_err(|e| err("open token", e))?;
        let mut luid = LUID::default();
        let looked_up = LookupPrivilegeValueW(None, SE_SHUTDOWN_NAME, &mut luid);
        let adjusted = looked_up.and_then(|_| {
            let privileges = TOKEN_PRIVILEGES {
                PrivilegeCount: 1,
                Privileges: [LUID_AND_ATTRIBUTES {
                    Luid: luid,
                    Attributes: SE_PRIVILEGE_ENABLED,
                }],
            };
            AdjustTokenPrivileges(token, false, Some(&privileges), 0, None, None)
        });
        // Succeeds even when the privilege is not held; that case is
        // reported through the last error.
        let not_held = GetLastError() == ERROR_NOT_ALL_ASSIGNED;
        let _ = CloseHandle(token);
        adjusted.map_err(|e| err("enable privilege", e))?;
        if not_held {
            return Err(AppError::msg("sleep: this account may not shut down or sleep the PC"));
        }
    }
    Ok(())
}

#[cfg(windows)]
fn shutdown_exe(args: &[&str]) -> AppResult<()> {
    use std::os::windows::process::CommandExt;
    const CREATE_NO_WINDOW: u32 = 0x0800_0000;
    let status = std::process::Command::new("shutdown.exe")
        .args(args)
        .creation_flags(CREATE_NO_WINDOW)
        .status()?;
    if status.success() {
        Ok(())
    } else {
        Err(AppError::msg(format!("shutdown.exe exited with {status}")))
    }
}

#[cfg(not(windows))]
pub fn run(_action: PowerAction) -> AppResult<()> {
    Err(AppError::msg("power actions are only available on Windows"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_actions() {
        let a: PowerAction = serde_json::from_str(r#""signOut""#).unwrap();
        assert_eq!(a, PowerAction::SignOut);
        assert!(serde_json::from_str::<PowerAction>(r#""format-c""#).is_err());
    }
}
