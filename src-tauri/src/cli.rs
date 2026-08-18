use serde::Serialize;
use std::path::Path;
use std::process::{Child, Command, Stdio};
use std::sync::OnceLock;
use std::time::{Duration, Instant};

#[cfg(windows)]
use std::os::windows::process::CommandExt;

#[cfg(windows)]
const CREATE_NO_WINDOW: u32 = 0x08000000;

const STREAM_CAP: usize = 8 * 1024;
/// Launcher inline runner — long jobs belong in terminal mode.
const EXEC_TIMEOUT: Duration = Duration::from_secs(4);
const SEARCH_BUDGET: Duration = Duration::from_secs(2);
const SEARCH_MAX_DEPTH: usize = 6;
const SEARCH_MAX_RESULTS: usize = 50;
const SKIP_DIRS: &[&str] = &[".git", "node_modules", "appdata", ".cursor", "target"];

#[derive(Serialize)]
pub struct CliOutput {
    pub stdout: String,
    pub stderr: String,
    pub code: i32,
}

#[derive(Serialize)]
pub struct SysStats {
    pub cpu: f32,
    pub mem: f32,
    pub disk: f32,
}

fn cap_stream(bytes: &[u8]) -> String {
    let s = String::from_utf8_lossy(bytes);
    if s.len() <= STREAM_CAP {
        return s.into_owned();
    }
    let mut end = STREAM_CAP;
    while !s.is_char_boundary(end) {
        end -= 1;
    }
    let mut out = s[..end].to_string();
    out.push('…');
    out
}

struct ShellConfig {
    exe: &'static str,
    /// Windows PowerShell 5.1 needs an explicit UTF-8 preamble; pwsh 7 does not.
    utf8_preamble: bool,
}

fn shell_config() -> Result<&'static ShellConfig, String> {
    static CONFIG: OnceLock<Result<ShellConfig, String>> = OnceLock::new();
    CONFIG
        .get_or_init(|| {
            let probe = |exe: &str| -> bool {
                let mut c = Command::new(exe);
                c.args(["-NoProfile", "-NoLogo", "-NonInteractive", "-Command", "exit 0"])
                    .stdin(Stdio::null())
                    .stdout(Stdio::null())
                    .stderr(Stdio::null());
                #[cfg(windows)]
                c.creation_flags(CREATE_NO_WINDOW);
                c.status().map(|s| s.success()).unwrap_or(false)
            };
            if probe("pwsh") {
                Ok(ShellConfig {
                    exe: "pwsh",
                    utf8_preamble: false,
                })
            } else if probe("powershell") {
                Ok(ShellConfig {
                    exe: "powershell",
                    utf8_preamble: true,
                })
            } else {
                Err("no usable shell found (pwsh/powershell)".to_string())
            }
        })
        .as_ref()
        .map_err(|e| e.clone())
}

/// Preferred interactive/shell binary for PTY sessions.
pub fn shell_exe() -> Result<&'static str, String> {
    Ok(shell_config()?.exe)
}

fn apply_hidden(child: &mut Command) {
    #[cfg(windows)]
    child.creation_flags(CREATE_NO_WINDOW);
}

fn spawn_shell(script: &str, cwd: &Option<String>) -> Result<Child, String> {
    let cfg = shell_config()?;
    let script = if cfg.utf8_preamble {
        format!("[Console]::OutputEncoding = [System.Text.Encoding]::UTF8; {script}")
    } else {
        script.to_string()
    };
    let mut c = Command::new(cfg.exe);
    // Must pipe stdout/stderr — wait_with_output() only captures piped streams.
    // CREATE_NO_WINDOW keeps pwsh invisible (no flash, no focus steal on Windows).
    c.args(["-NoProfile", "-NoLogo", "-NonInteractive", "-Command", &script])
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    apply_hidden(&mut c);
    if let Some(dir) = cwd {
        if Path::new(dir).is_dir() {
            c.current_dir(dir);
        }
    }
    c.spawn()
        .map_err(|e| format!("failed to spawn {}: {e}", cfg.exe))
}

#[tauri::command]
pub fn cli_exec(cmd: String, cwd: Option<String>) -> Result<CliOutput, String> {
    let mut child = spawn_shell(&cmd, &cwd)?;
    let stdout_pipe = child
        .stdout
        .take()
        .ok_or_else(|| "failed to capture shell stdout".to_string())?;
    let stderr_pipe = child
        .stderr
        .take()
        .ok_or_else(|| "failed to capture shell stderr".to_string())?;

    let stdout_handle = std::thread::spawn(move || {
        use std::io::Read;
        let mut buf = Vec::new();
        let mut pipe = stdout_pipe;
        let _ = pipe.read_to_end(&mut buf);
        buf
    });
    let stderr_handle = std::thread::spawn(move || {
        use std::io::Read;
        let mut buf = Vec::new();
        let mut pipe = stderr_pipe;
        let _ = pipe.read_to_end(&mut buf);
        buf
    });

    let deadline = Instant::now() + EXEC_TIMEOUT;
    let status = loop {
        match child.try_wait() {
            Ok(Some(status)) => break status,
            Ok(None) => {
                if Instant::now() >= deadline {
                    let _ = child.kill();
                    let _ = child.wait();
                    return Err("command timed out after 4s".to_string());
                }
                std::thread::sleep(Duration::from_millis(20));
            }
            Err(e) => return Err(format!("failed to wait for shell: {e}")),
        }
    };

    let stdout = cap_stream(&stdout_handle.join().unwrap_or_default());
    let stderr = cap_stream(&stderr_handle.join().unwrap_or_default());
    let code = status.code().unwrap_or(-1);

    Ok(CliOutput {
        stdout,
        stderr,
        code,
    })
}

/// Launch a visible terminal (Windows Terminal when available) for interactive or long-running work.
#[tauri::command]
pub fn cli_exec_term(cmd: String, cwd: Option<String>, interactive: bool) -> Result<(), String> {
    let cfg = shell_config()?;
    let cmd = cmd.trim();
    if !interactive && cmd.is_empty() {
        return Err("cli_exec_term: empty command".to_string());
    }
    let cwd = cwd.filter(|d| Path::new(d).is_dir());

    #[cfg(windows)]
    {
        if try_windows_terminal(cfg, cmd, &cwd, interactive).is_ok() {
            return Ok(());
        }
        return try_cmd_start_terminal(cfg, cmd, &cwd, interactive);
    }

    #[cfg(not(windows))]
    {
        let mut c = Command::new(cfg.exe);
        c.args(["-NoProfile", "-NoLogo"]);
        if interactive {
            c.arg("-NoExit");
        } else {
            c.args(["-Command", cmd]);
        }
        if let Some(ref dir) = cwd {
            c.current_dir(dir);
        }
        c.spawn()
            .map(|_| ())
            .map_err(|e| format!("failed to open terminal: {e}"))
    }
}

#[cfg(windows)]
fn try_windows_terminal(
    cfg: &ShellConfig,
    cmd: &str,
    cwd: &Option<String>,
    interactive: bool,
) -> Result<(), String> {
    let mut wt = Command::new("wt");
    if let Some(dir) = cwd {
        wt.arg("-d").arg(dir);
    }
    wt.arg(cfg.exe).args(["-NoProfile", "-NoLogo"]);
    if interactive {
        wt.arg("-NoExit");
    } else {
        wt.args(["-Command", cmd]);
    }
    apply_hidden(&mut wt);
    wt.spawn()
        .map(|_| ())
        .map_err(|e| format!("Windows Terminal (wt): {e}"))
}

#[cfg(windows)]
fn try_cmd_start_terminal(
    cfg: &ShellConfig,
    cmd: &str,
    cwd: &Option<String>,
    interactive: bool,
) -> Result<(), String> {
    let mut args = vec!["/C".to_string(), "start".to_string()];
    if let Some(dir) = cwd {
        args.push(format!("/D{dir}"));
    }
    args.push("".to_string());
    args.push(cfg.exe.to_string());
    args.push("-NoProfile".to_string());
    args.push("-NoLogo".to_string());
    if interactive || cmd.is_empty() {
        args.push("-NoExit".to_string());
    } else {
        args.push("-Command".to_string());
        args.push(cmd.to_string());
    }
    let mut c = Command::new("cmd");
    c.args(args);
    apply_hidden(&mut c);
    c.spawn()
        .map(|_| ())
        .map_err(|e| format!("failed to open terminal: {e}"))
}

#[cfg(windows)]
fn open_target(target: &str) -> Result<(), String> {
    use std::ffi::OsStr;
    use std::os::windows::ffi::OsStrExt;
    use windows::core::PCWSTR;
    use windows::Win32::UI::Shell::ShellExecuteW;
    use windows::Win32::UI::WindowsAndMessaging::SW_SHOWNORMAL;

    let to_wide = |s: &str| -> Vec<u16> {
        OsStr::new(s).encode_wide().chain(std::iter::once(0)).collect()
    };
    let verb = to_wide("open");
    let file = to_wide(target);
    // ShellExecuteW returns an HINSTANCE cast; values > 32 mean success.
    let rc = unsafe {
        ShellExecuteW(
            None,
            PCWSTR(verb.as_ptr()),
            PCWSTR(file.as_ptr()),
            PCWSTR::null(),
            PCWSTR::null(),
            SW_SHOWNORMAL,
        )
    };
    let code = rc.0 as isize;
    if code > 32 {
        Ok(())
    } else {
        Err(format!("failed to open '{target}' (ShellExecute={code})"))
    }
}

#[cfg(not(windows))]
fn open_target(target: &str) -> Result<(), String> {
    Command::new("xdg-open")
        .arg(target)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .map(|_| ())
        .map_err(|e| format!("failed to open '{target}': {e}"))
}

#[tauri::command]
pub fn cli_home() -> Result<String, String> {
    dirs::home_dir()
        .map(|p| p.to_string_lossy().into_owned())
        .ok_or_else(|| "could not resolve home directory".to_string())
}

#[tauri::command]
pub fn cli_open(target: String) -> Result<(), String> {
    let resolved = crate::apps::resolve_launch_target(&target)?;
    open_target(&resolved)
}

#[tauri::command]
pub fn cli_search_files(query: String) -> Result<Vec<String>, String> {
    let q = query.trim().to_lowercase();
    if q.is_empty() {
        return Ok(Vec::new());
    }
    let home = dirs::home_dir().ok_or_else(|| "could not resolve home directory".to_string())?;
    let start = Instant::now();
    let mut results: Vec<String> = Vec::new();

    // Prefer user-facing folders first so the 2s budget isn't burned in deep trees.
    let mut roots: Vec<std::path::PathBuf> = [
        home.join("Documents"),
        home.join("Desktop"),
        home.join("Downloads"),
        home.join("Pictures"),
        home.clone(),
    ]
    .into_iter()
    .filter(|p| p.is_dir())
    .collect();
    roots.dedup();

    // BFS — shallow matches beat deep noise for launcher UX.
    let mut queue: std::collections::VecDeque<(std::path::PathBuf, usize)> = roots
        .into_iter()
        .map(|p| (p, 0))
        .collect();
    let mut seen = std::collections::HashSet::new();

    while let Some((dir, depth)) = queue.pop_front() {
        if start.elapsed() > SEARCH_BUDGET || results.len() >= SEARCH_MAX_RESULTS {
            break;
        }
        if depth > SEARCH_MAX_DEPTH {
            continue;
        }
        let key = dir.to_string_lossy().to_lowercase();
        if !seen.insert(key) {
            continue;
        }
        let rd = match std::fs::read_dir(&dir) {
            Ok(r) => r,
            Err(_) => continue,
        };
        for entry in rd.flatten() {
            if start.elapsed() > SEARCH_BUDGET || results.len() >= SEARCH_MAX_RESULTS {
                break;
            }
            let name = entry.file_name().to_string_lossy().into_owned();
            let lower = name.to_lowercase();
            let file_type = match entry.file_type() {
                Ok(t) => t,
                Err(_) => continue,
            };
            let path = entry.path();
            if lower.contains(&q) {
                results.push(path.to_string_lossy().into_owned());
            }
            if file_type.is_dir() && !SKIP_DIRS.contains(&lower.as_str()) {
                queue.push_back((path, depth + 1));
            }
        }
    }

    results.sort_by_key(|p| {
        Path::new(p)
            .file_name()
            .map(|n| n.to_string_lossy().to_lowercase())
            .unwrap_or_default()
    });
    results.truncate(SEARCH_MAX_RESULTS);
    Ok(results)
}

#[tauri::command]
pub fn sys_stats() -> Result<SysStats, String> {
    use sysinfo::{CpuRefreshKind, Disks, MemoryRefreshKind, RefreshKind, System};

    let mut sys = System::new_with_specifics(
        RefreshKind::new()
            .with_cpu(CpuRefreshKind::everything())
            .with_memory(MemoryRefreshKind::everything()),
    );
    // CPU usage is a delta between two refreshes.
    sys.refresh_cpu_usage();
    std::thread::sleep(Duration::from_millis(200));
    sys.refresh_cpu_usage();
    let cpu = sys.global_cpu_info().cpu_usage();

    sys.refresh_memory();
    let total_mem = sys.total_memory();
    let mem = if total_mem > 0 {
        (sys.used_memory() as f32 / total_mem as f32) * 100.0
    } else {
        0.0
    };

    let disks = Disks::new_with_refreshed_list();
    let disk = disks
        .iter()
        .max_by_key(|d| d.total_space())
        .map(|d| {
            let total = d.total_space();
            if total > 0 {
                ((total - d.available_space()) as f32 / total as f32) * 100.0
            } else {
                0.0
            }
        })
        .unwrap_or(0.0);

    Ok(SysStats { cpu, mem, disk })
}

/// Taskbar "Show desktop" (Win+D). Lives in the action bar, not the HUD.
pub fn show_desktop() -> Result<(), String> {
    #[cfg(windows)]
    {
        use windows::Win32::UI::Input::KeyboardAndMouse::{
            SendInput, INPUT, INPUT_0, INPUT_KEYBOARD, KEYBDINPUT, KEYEVENTF_KEYUP, VK_D,
            VK_LWIN,
        };
        let key = |vk, up: bool| INPUT {
            r#type: INPUT_KEYBOARD,
            Anonymous: INPUT_0 {
                ki: KEYBDINPUT {
                    wVk: vk,
                    wScan: 0,
                    dwFlags: if up {
                        KEYEVENTF_KEYUP
                    } else {
                        Default::default()
                    },
                    time: 0,
                    dwExtraInfo: 0,
                },
            },
        };
        let inputs = [
            key(VK_LWIN, false),
            key(VK_D, false),
            key(VK_D, true),
            key(VK_LWIN, true),
        ];
        let sent = unsafe { SendInput(&inputs, std::mem::size_of::<INPUT>() as i32) };
        if sent == 0 {
            return Err("show desktop: SendInput failed".into());
        }
        Ok(())
    }
    #[cfg(not(windows))]
    {
        Err("show desktop is Windows-only".into())
    }
}
