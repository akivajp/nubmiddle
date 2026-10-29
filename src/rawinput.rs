//! Raw Input: tells which device a mouse event came from (the low-level hook cannot).
//!
//! Mouse raw input is registered for the hidden tray window with `RIDEV_INPUTSINK`, so
//! `WM_INPUT` arrives on the hook thread even while other windows have the focus.
//! For now this only logs the events with `--trace`, to check how they line up with the
//! hook's events (order, and whether events swallowed by the hook still produce raw input).

// On Linux only the tests use the helpers
#![cfg_attr(not(windows), allow(dead_code))]

/// Extracts "VVVV:PPPP" (vendor and product id, upper case) from a device interface name such as
/// `\\?\HID#VID_17EF&PID_6047&MI_01&Col01#...` (USB) or `...VID&0217ef_PID&6047...`
/// (Bluetooth LE, with a 2-digit id source before the vendor id).
/// None for names without ids (e.g. the RDP mouse).
pub fn vid_pid(name: &str) -> Option<String> {
    let upper = name.to_ascii_uppercase();
    // `skip` digits are dropped before the 4-digit id
    let id = |key: &str, skip: usize| {
        let start = upper.find(key)? + key.len() + skip;
        let hex = upper.get(start..start + 4)?;
        hex.chars()
            .all(|c| c.is_ascii_hexdigit())
            .then(|| hex.to_string())
    };
    let vid = id("VID_", 0).or_else(|| id("VID&", 2))?;
    let pid = id("PID_", 0).or_else(|| id("PID&", 0))?;
    Some(format!("{vid}:{pid}"))
}

#[cfg(windows)]
pub use self::windows::*;

#[cfg(windows)]
mod windows {
    use std::cell::RefCell;
    use std::collections::HashMap;

    use windows_sys::Win32::Foundation::{HANDLE, HWND, LPARAM};
    use windows_sys::Win32::UI::Input::{
        GetRawInputData, GetRawInputDeviceInfoW, GetRawInputDeviceList, HRAWINPUT, RAWINPUT,
        RAWINPUTDEVICE, RAWINPUTDEVICELIST, RAWINPUTHEADER, RID_INPUT, RIDEV_INPUTSINK,
        RIDI_DEVICENAME, RIM_TYPEMOUSE, RegisterRawInputDevices,
    };
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        RI_MOUSE_MIDDLE_BUTTON_DOWN, RI_MOUSE_MIDDLE_BUTTON_UP,
    };

    use super::vid_pid;

    /// One mouse raw input event.
    #[derive(Debug, Clone, Copy)]
    pub struct RawMouse {
        pub device: HANDLE,
        /// `RI_MOUSE_*` button transition flags
        pub buttons: u16,
        pub dx: i32,
        pub dy: i32,
    }

    impl RawMouse {
        pub fn middle_down(&self) -> bool {
            u32::from(self.buttons) & RI_MOUSE_MIDDLE_BUTTON_DOWN != 0
        }
        pub fn middle_up(&self) -> bool {
            u32::from(self.buttons) & RI_MOUSE_MIDDLE_BUTTON_UP != 0
        }
    }

    thread_local! {
        // Device names by handle (asking the system on every event would be wasteful)
        static NAMES: RefCell<HashMap<usize, String>> = RefCell::new(HashMap::new());
    }

    /// Registers mouse raw input for `hwnd`, also while it is in the background.
    pub fn register(hwnd: HWND) -> Result<(), String> {
        let device = RAWINPUTDEVICE {
            usUsagePage: 0x01, // generic desktop
            usUsage: 0x02,     // mouse
            dwFlags: RIDEV_INPUTSINK,
            hwndTarget: hwnd,
        };
        let ok = unsafe {
            RegisterRawInputDevices(&device, 1, std::mem::size_of::<RAWINPUTDEVICE>() as u32)
        };
        if ok == 0 {
            return Err(format!(
                "RegisterRawInputDevices failed: {}",
                std::io::Error::last_os_error()
            ));
        }
        Ok(())
    }

    /// Reads the mouse event of a `WM_INPUT` message (None for other devices or on errors).
    pub fn read(lparam: LPARAM) -> Option<RawMouse> {
        let mut raw: RAWINPUT = unsafe { std::mem::zeroed() };
        let mut size = std::mem::size_of::<RAWINPUT>() as u32;
        let header = std::mem::size_of::<RAWINPUTHEADER>() as u32;
        let n = unsafe {
            GetRawInputData(
                lparam as HRAWINPUT,
                RID_INPUT,
                (&mut raw as *mut RAWINPUT).cast(),
                &mut size,
                header,
            )
        };
        if n == u32::MAX || raw.header.dwType != RIM_TYPEMOUSE {
            return None;
        }
        let mouse = unsafe { raw.data.mouse };
        Some(RawMouse {
            device: raw.header.hDevice,
            buttons: unsafe { mouse.Anonymous.Anonymous.usButtonFlags },
            dx: mouse.lLastX,
            dy: mouse.lLastY,
        })
    }

    /// Interface name of a device (e.g. `\\?\HID#VID_17EF&PID_6047...`), cached.
    pub fn device_name(device: HANDLE) -> String {
        if device.is_null() {
            // Injected input (SendInput) has no device
            return "(none)".into();
        }
        NAMES.with(|names| {
            names
                .borrow_mut()
                .entry(device as usize)
                .or_insert_with(|| query_name(device))
                .clone()
        })
    }

    /// Asks the system for a device's interface name.
    fn query_name(device: HANDLE) -> String {
        let mut len = 0u32;
        unsafe {
            GetRawInputDeviceInfoW(device, RIDI_DEVICENAME, std::ptr::null_mut(), &mut len);
            let mut buf = vec![0u16; len as usize];
            let n =
                GetRawInputDeviceInfoW(device, RIDI_DEVICENAME, buf.as_mut_ptr().cast(), &mut len);
            if n == u32::MAX {
                return format!("(handle {:#x})", device as usize);
            }
            let end = buf.iter().position(|&c| c == 0).unwrap_or(buf.len());
            String::from_utf16_lossy(&buf[..end])
        }
    }

    /// Id used in the settings: "VVVV:PPPP", or the interface name for devices without ids.
    /// None for injected input, which has no device.
    pub fn device_id(device: HANDLE) -> Option<String> {
        if device.is_null() {
            return None;
        }
        let name = device_name(device);
        Some(vid_pid(&name).unwrap_or(name))
    }

    /// Short description of a device for logs: "VVVV:PPPP" plus its interface name.
    pub fn describe(device: HANDLE) -> String {
        let name = device_name(device);
        match vid_pid(&name) {
            Some(id) => format!("{id} {name}"),
            None => name,
        }
    }

    /// Product name reported by the HID device (e.g. "ThinkPad Compact USB Keyboard with
    /// TrackPoint"), or None if it cannot be read. Opens the device without access rights,
    /// which is allowed even for mice that the system holds exclusively.
    pub fn product_name(device: HANDLE) -> Option<String> {
        use windows_sys::Win32::Devices::HumanInterfaceDevice::HidD_GetProductString;
        use windows_sys::Win32::Foundation::{CloseHandle, INVALID_HANDLE_VALUE};
        use windows_sys::Win32::Storage::FileSystem::{
            CreateFileW, FILE_SHARE_READ, FILE_SHARE_WRITE, OPEN_EXISTING,
        };

        let name: Vec<u16> = device_name(device).encode_utf16().chain(Some(0)).collect();
        unsafe {
            let file = CreateFileW(
                name.as_ptr(),
                0,
                FILE_SHARE_READ | FILE_SHARE_WRITE,
                std::ptr::null(),
                OPEN_EXISTING,
                0,
                std::ptr::null_mut(),
            );
            if file == INVALID_HANDLE_VALUE {
                return None;
            }
            // HID strings are at most 126 characters plus the terminator
            let mut buf = [0u16; 127];
            let ok = HidD_GetProductString(file, buf.as_mut_ptr().cast(), (buf.len() * 2) as u32);
            CloseHandle(file);
            if !ok {
                return None;
            }
            let end = buf.iter().position(|&c| c == 0).unwrap_or(buf.len());
            let product = String::from_utf16_lossy(&buf[..end]).trim().to_string();
            (!product.is_empty()).then_some(product)
        }
    }

    /// Connected devices that can be chosen in the menu: (id, product name), one per id
    /// (a device often shows up once per HID collection).
    pub fn connected() -> Vec<(String, Option<String>)> {
        let mut out: Vec<(String, Option<String>)> = Vec::new();
        for device in list_mice() {
            let Some(id) = device_id(device) else {
                continue;
            };
            match out.iter_mut().find(|(known, _)| *known == id) {
                // Keep the first readable product name
                Some((_, name @ None)) => *name = product_name(device),
                Some(_) => {}
                None => out.push((id, product_name(device))),
            }
        }
        out
    }

    /// Lists the connected mouse-class devices (pointing sticks, touchpads and mice alike).
    pub fn list_mice() -> Vec<HANDLE> {
        let size = std::mem::size_of::<RAWINPUTDEVICELIST>() as u32;
        let mut count = 0u32;
        unsafe {
            GetRawInputDeviceList(std::ptr::null_mut(), &mut count, size);
            let mut list = vec![RAWINPUTDEVICELIST::default(); count as usize];
            let n = GetRawInputDeviceList(list.as_mut_ptr(), &mut count, size);
            if n == u32::MAX {
                return vec![];
            }
            list.truncate(n as usize);
            list.into_iter()
                .filter(|d| d.dwType == RIM_TYPEMOUSE)
                .map(|d| d.hDevice)
                .collect()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn vid_pid_from_interface_names() {
        assert_eq!(
            vid_pid(
                r"\\?\HID#VID_17EF&PID_6047&MI_01&Col01#8&1234&0&0000#{378de44c-56ef-11d1-bc8c-00a0c91405dd}"
            ),
            Some("17EF:6047".into())
        );
        // Bluetooth LE devices spell it differently, with an id source before the vendor id
        assert_eq!(
            vid_pid(
                r"\\?\HID#{00001812-0000-1000-8000-00805f9b34fb}_Dev_VID&02046d_PID&b023&Col01#a"
            ),
            Some("046D:B023".into())
        );
        assert_eq!(
            vid_pid(r"\\?\hid#vid_04fe&pid_0021&mi_02#7&abc"),
            Some("04FE:0021".into())
        );
        assert_eq!(vid_pid(r"\\?\Root#RDP_MOU#0000#{378de44c}"), None);
    }
}
