//! Inspecting process elevation and the process owning a window (Windows only).

use std::ptr::null_mut;

use windows_sys::Win32::Foundation::{CloseHandle, HANDLE, HWND, POINT};
use windows_sys::Win32::Security::{
    GetTokenInformation, TOKEN_ELEVATION, TOKEN_QUERY, TokenElevation,
};
use windows_sys::Win32::System::Threading::{
    GetCurrentProcess, OpenProcess, OpenProcessToken, PROCESS_NAME_WIN32,
    PROCESS_QUERY_LIMITED_INFORMATION, QueryFullProcessImageNameW,
};
use windows_sys::Win32::UI::WindowsAndMessaging::{
    GA_ROOT, GetAncestor, GetWindowThreadProcessId, WindowFromPoint,
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

/// Describes the process owning the top-level window at screen coordinates (x, y)
/// as "exe name (pid=..., elevated=...)". For troubleshooting.
pub fn describe_window_at(x: i32, y: i32) -> String {
    unsafe {
        let hwnd: HWND = GetAncestor(WindowFromPoint(POINT { x, y }), GA_ROOT);
        if hwnd.is_null() {
            return "(no window)".into();
        }
        let mut pid = 0u32;
        GetWindowThreadProcessId(hwnd, &mut pid);
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
