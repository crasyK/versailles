//! Hide helper HWNDs that Windows shows as ~15x15 squares at (0,0).
//!
//! Both `tauri-plugin-single-instance` and `tao` create WS_VISIBLE | WS_POPUP
//! layered windows without SetLayeredWindowAttributes, so they can appear as
//! upright squares on the desktop / taskbar.

#[cfg(windows)]
fn make_layered_invisible(hwnd: windows::Win32::Foundation::HWND) {
    use windows::Win32::Foundation::COLORREF;
    use windows::Win32::UI::WindowsAndMessaging::{SetLayeredWindowAttributes, LWA_ALPHA};

    unsafe {
        let _ = SetLayeredWindowAttributes(hwnd, COLORREF(0), 0, LWA_ALPHA);
    }
}

#[cfg(windows)]
fn add_toolwindow_exstyle(hwnd: windows::Win32::Foundation::HWND) {
    use windows::Win32::UI::WindowsAndMessaging::{
        GetWindowLongW, SetWindowLongW, GWL_EXSTYLE, WS_EX_TOOLWINDOW,
    };

    unsafe {
        let ex = GetWindowLongW(hwnd, GWL_EXSTYLE);
        let _ = SetWindowLongW(hwnd, GWL_EXSTYLE, ex | WS_EX_TOOLWINDOW.0 as i32);
    }
}

#[cfg(windows)]
fn delete_taskbar_tab(hwnd: windows::Win32::Foundation::HWND) {
    use windows::Win32::System::Com::{
        CoCreateInstance, CoInitializeEx, CLSCTX_INPROC_SERVER, COINIT_APARTMENTTHREADED,
    };
    use windows::Win32::UI::Shell::{ITaskbarList, TaskbarList};

    unsafe {
        let _ = CoInitializeEx(None, COINIT_APARTMENTTHREADED);
        match CoCreateInstance::<_, ITaskbarList>(&TaskbarList, None, CLSCTX_INPROC_SERVER) {
            Ok(taskbar) => {
                if let Err(err) = taskbar.HrInit() {
                    tracing::warn!("ITaskbarList::HrInit failed: {err}");
                    return;
                }
                if let Err(err) = taskbar.DeleteTab(hwnd) {
                    tracing::warn!("ITaskbarList::DeleteTab failed: {err}");
                } else {
                    tracing::info!("removed taskbar tab for helper HWND");
                }
            }
            Err(err) => {
                tracing::warn!("CoCreateInstance(ITaskbarList) failed: {err}");
            }
        }
    }
}

/// Hide / de-taskbar the single-instance and Tao helper windows.
#[cfg(windows)]
pub fn hide_single_instance_helper() {
    use windows::core::w;
    use windows::Win32::Foundation::RECT;
    use windows::Win32::UI::WindowsAndMessaging::{
        FindWindowW, GetWindowRect, IsWindowVisible, ShowWindow, SW_HIDE,
    };

    unsafe {
        // Single-instance IPC target - safe to fully hide; WM_COPYDATA still works.
        match FindWindowW(w!("com.versailles.widgets-sic"), w!("com.versailles.widgets-siw")) {
            Ok(hwnd) => {
                let mut rect = RECT::default();
                let _ = GetWindowRect(hwnd, &mut rect);
                let _vis = IsWindowVisible(hwnd).as_bool();
                make_layered_invisible(hwnd);
                add_toolwindow_exstyle(hwnd);
                delete_taskbar_tab(hwnd);
                let _ = ShowWindow(hwnd, SW_HIDE);
                tracing::info!("hid helper window (single-instance)");
            }
            Err(_) => {
                tracing::debug!("single-instance helper HWND not found");
            }
        }

        // tao event-loop target must stay "visible" for WM_PAINT; do not SW_HIDE.
        // Park it off-screen + fully transparent so it never greets the user as a speck/console.
        match FindWindowW(w!("Tao Thread Event Target"), None) {
            Ok(hwnd) => {
                make_layered_invisible(hwnd);
                add_toolwindow_exstyle(hwnd);
                delete_taskbar_tab(hwnd);
                use windows::Win32::UI::WindowsAndMessaging::{
                    SetWindowPos, SWP_NOACTIVATE, SWP_NOSIZE, SWP_NOZORDER,
                };
                let _ = SetWindowPos(
                    hwnd,
                    None,
                    -32000,
                    -32000,
                    0,
                    0,
                    SWP_NOSIZE | SWP_NOZORDER | SWP_NOACTIVATE,
                );
                tracing::info!("soft-hid helper window (tao-event-target)");
            }
            Err(_) => {
                tracing::debug!("tao event-target HWND not found");
            }
        }
    }
}

#[cfg(not(windows))]
pub fn hide_single_instance_helper() {}

/// Hide helpers immediately, then retry after 100ms / 500ms / 2000ms in case
/// the HWNDs appear after plugin/tao setup finishes.
pub fn schedule_hide_helpers() {
    hide_single_instance_helper();
    std::thread::spawn(|| {
        for delay_ms in [100u64, 500, 2000, 8000, 20000] {
            std::thread::sleep(std::time::Duration::from_millis(delay_ms));
            hide_single_instance_helper();
        }
    });
}
