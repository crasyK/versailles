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
            // Suspend (not hibernate), let apps veto, keep wake events.
            if unsafe { SetSuspendState(false, false, false) } {
                Ok(())
            } else {
                Err(AppError::msg("sleep was refused"))
            }
        }
        PowerAction::SignOut => unsafe { ExitWindowsEx(EWX_LOGOFF, SHTDN_REASON_MAJOR_OTHER) }.map_err(err),
        // shutdown.exe holds the shutdown privilege handling for us.
        PowerAction::Restart => shutdown_exe(&["/r", "/t", "0"]),
        PowerAction::Shutdown => shutdown_exe(&["/s", "/t", "0"]),
    }
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
