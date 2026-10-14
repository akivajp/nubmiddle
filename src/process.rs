//! Inspecting process elevation and the process owning a window (Windows only).

use std::ptr::{null, null_mut};

use windows_sys::Win32::Foundation::{CloseHandle, HANDLE, HWND, POINT};
use windows_sys::Win32::Security::{
    GetTokenInformation, TOKEN_ELEVATION, TOKEN_QUERY, TokenElevation,
};
use windows_sys::Win32::System::Threading::{
    GetCurrentProcess, OpenProcess, OpenProcessToken, PROCESS_NAME_WIN32,
    PROCESS_QUERY_LIMITED_INFORMATION, QueryFullProcessImageNameW,
};
use windows_sys::Win32::UI::WindowsAndMessaging::{
    FindWindowExW, GA_ROOT, GetAncestor, GetClassNameW, GetForegroundWindow, GetWindowTextW,
    GetWindowThreadProcessId, WindowFromPoint,
};

/// Returns whether this process runs as administrator (elevated token).
pub fn is_current_elevated() -> bool {
    token_elevated(unsafe { GetCurrentProcess() }) == Some(true)
}

/// Returns whether the process token is elevated, or None if the token cannot be opened.
fn token_elevated(process: HANDLE) -> Option<bool> {
    unsafe {
        let mut token: HANDLE = null_mut();
        if OpenProcessToken(process, TOKEN_QUERY, &mut token) == 0 {
            return None;
        }
        let mut elevation = TOKEN_ELEVATION { TokenIsElevated: 0 };
        let mut len = 0u32;
        let ok = GetTokenInformation(
            token,
            TokenElevation,
            (&mut elevation as *mut TOKEN_ELEVATION).cast(),
            std::mem::size_of::<TOKEN_ELEVATION>() as u32,
            &mut len,
        );
        CloseHandle(token);
        (ok != 0).then_some(elevation.TokenIsElevated != 0)
    }
}

/// Describes the windows at screen coordinates (x, y) for troubleshooting:
/// the top-level window (process, class, title), the child window under the point,
/// the real app inside a UWP frame (`ApplicationFrameWindow`) and the foreground window.
pub fn describe_window_at(x: i32, y: i32) -> String {
    unsafe {
        let child: HWND = WindowFromPoint(POINT { x, y });
        let root: HWND = GetAncestor(child, GA_ROOT);
        if root.is_null() {
            return "(no window)".into();
        }
        let mut line = format!(
            "{} class={} title={:?}",
            describe_process(window_pid(root)),
            window_class(root),
            window_title(root)
        );
        // The child under the point may belong to another process (e.g. hosted content)
        if child != root {
            line += &format!(
                " child={} class={}",
                describe_process(window_pid(child)),
                window_class(child)
            );
        }
        // UWP apps run in a separate process inside a frame owned by ApplicationFrameHost.exe
        if window_class(root) == "ApplicationFrameWindow" {
            let class = wide("Windows.UI.Core.CoreWindow");
            let core = FindWindowExW(root, null_mut(), class.as_ptr(), null());
            if !core.is_null() {
                line += &format!(" app={}", describe_process(window_pid(core)));
            }
        }
        // Wheel messages may go to the focused window instead of the one under the cursor
        let fg = GetForegroundWindow();
        if fg != root {
            line += &format!(
                " foreground={} class={} title={:?}",
                describe_process(window_pid(fg)),
                window_class(fg),
                window_title(fg)
            );
        }
        line
    }
}

/// Returns the id of the process owning the window.
fn window_pid(hwnd: HWND) -> u32 {
    let mut pid = 0u32;
    unsafe { GetWindowThreadProcessId(hwnd, &mut pid) };
    pid
}

/// Returns the window class name.
fn window_class(hwnd: HWND) -> String {
    let mut buf = [0u16; 256];
    let len = unsafe { GetClassNameW(hwnd, buf.as_mut_ptr(), buf.len() as i32) };
    String::from_utf16_lossy(&buf[..len.max(0) as usize])
}

/// Returns the window title (truncated to 255 characters).
fn window_title(hwnd: HWND) -> String {
    let mut buf = [0u16; 256];
    let len = unsafe { GetWindowTextW(hwnd, buf.as_mut_ptr(), buf.len() as i32) };
    String::from_utf16_lossy(&buf[..len.max(0) as usize])
}

/// Converts a string to a NUL-terminated UTF-16 buffer.
fn wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(std::iter::once(0)).collect()
}

/// Describes a process as "exe name (pid=..., elevated=...)".
fn describe_process(pid: u32) -> String {
    unsafe {
        let process = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, pid);
        if process.is_null() {
            return format!("(pid={pid}, cannot open process)");
        }
        // Take only the file name from the full exe path
        let mut buf = [0u16; 260];
        let mut len = buf.len() as u32;
        let name =
            if QueryFullProcessImageNameW(process, PROCESS_NAME_WIN32, buf.as_mut_ptr(), &mut len)
                != 0
            {
                let path = String::from_utf16_lossy(&buf[..len as usize]);
                path.rsplit('\\').next().unwrap_or_default().to_string()
            } else {
                "?".into()
            };
        // A non-elevated process usually cannot open an elevated process's token (= likely elevated)
        let elevated = match token_elevated(process) {
            Some(true) => "yes",
            Some(false) => "no",
            None => "yes? (token access denied)",
        };
        CloseHandle(process);
        format!("{name} (pid={pid}, elevated={elevated})")
    }
}
