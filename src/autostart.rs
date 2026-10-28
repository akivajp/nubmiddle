//! Autostart at logon via Task Scheduler, with highest privileges.
//!
//! A scheduled task (not the Run registry key) is used because only a task can start the
//! process elevated without a UAC prompt at every logon, and an elevated process is needed
//! for the hook to work on elevated windows (UIPI).
//! The task is managed with `schtasks.exe` and an XML definition: tasks created with plain
//! `schtasks /SC ONLOGON` stop on battery power and after 72 hours, which a resident tool must avoid.
//!
//! The XML and command-line helpers are OS-independent so they can be tested on Linux.

// On Linux only the tests use the helpers
#![cfg_attr(not(windows), allow(dead_code))]

/// Name of the scheduled task.
pub const TASK_NAME: &str = "nubscroll";

/// Command line and arguments of a registered task.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TaskInfo {
    pub command: String,
    pub arguments: String,
}

/// Quotes one argument following the rules of `CommandLineToArgvW` / the MSVC runtime.
pub fn quote_arg(arg: &str) -> String {
    if !arg.is_empty() && !arg.contains([' ', '\t', '"']) {
        return arg.to_string();
    }
    let mut out = String::from('"');
    let mut backslashes = 0;
    for c in arg.chars() {
        match c {
            '\\' => backslashes += 1,
            '"' => {
                // Backslashes before a quote are escaped, then the quote itself
                out.extend(std::iter::repeat_n('\\', backslashes * 2 + 1));
                out.push('"');
                backslashes = 0;
            }
            _ => {
                out.extend(std::iter::repeat_n('\\', backslashes));
                out.push(c);
                backslashes = 0;
            }
        }
    }
    // Backslashes before the closing quote are escaped too
    out.extend(std::iter::repeat_n('\\', backslashes * 2));
    out.push('"');
    out
}

/// Joins arguments into one command-line string.
pub fn join_args(args: &[String]) -> String {
    args.iter()
        .map(|a| quote_arg(a))
        .collect::<Vec<_>>()
        .join(" ")
}

/// Escapes text for XML element content.
fn xml_escape(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&apos;")
}

/// Reverses [`xml_escape`].
fn xml_unescape(s: &str) -> String {
    s.replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&quot;", "\"")
        .replace("&apos;", "'")
        .replace("&amp;", "&")
}

/// Returns the (unescaped) text of the first `<tag>...</tag>` element, if any.
fn xml_text(xml: &str, tag: &str) -> Option<String> {
    let open = format!("<{tag}>");
    let close = format!("</{tag}>");
    let start = xml.find(&open)? + open.len();
    let end = start + xml[start..].find(&close)?;
    Some(xml_unescape(&xml[start..end]))
}

/// Builds the task definition: run `exe arguments` in `dir` at logon of `user`,
/// with highest privileges, without battery or time limits.
pub fn task_xml(user: &str, exe: &str, arguments: &str, dir: &str) -> String {
    let (user, exe, arguments, dir) = (
        xml_escape(user),
        xml_escape(exe),
        xml_escape(arguments),
        xml_escape(dir),
    );
    format!(
        r#"<?xml version="1.0" encoding="UTF-16"?>
<Task version="1.2" xmlns="http://schemas.microsoft.com/windows/2004/02/mit/task">
  <RegistrationInfo>
    <Description>Starts nubscroll at logon with administrator rights, so that it also works on elevated windows.</Description>
  </RegistrationInfo>
  <Triggers>
    <LogonTrigger>
      <Enabled>true</Enabled>
      <UserId>{user}</UserId>
    </LogonTrigger>
  </Triggers>
  <Principals>
    <Principal id="Author">
      <UserId>{user}</UserId>
      <LogonType>InteractiveToken</LogonType>
      <RunLevel>HighestAvailable</RunLevel>
    </Principal>
  </Principals>
  <Settings>
    <MultipleInstancesPolicy>IgnoreNew</MultipleInstancesPolicy>
    <DisallowStartIfOnBatteries>false</DisallowStartIfOnBatteries>
    <StopIfGoingOnBatteries>false</StopIfGoingOnBatteries>
    <AllowHardTerminate>true</AllowHardTerminate>
    <StartWhenAvailable>false</StartWhenAvailable>
    <RunOnlyIfNetworkAvailable>false</RunOnlyIfNetworkAvailable>
    <IdleSettings>
      <StopOnIdleEnd>false</StopOnIdleEnd>
      <RestartOnIdle>false</RestartOnIdle>
    </IdleSettings>
    <AllowStartOnDemand>true</AllowStartOnDemand>
    <Enabled>true</Enabled>
    <Hidden>false</Hidden>
    <RunOnlyIfIdle>false</RunOnlyIfIdle>
    <ExecutionTimeLimit>PT0S</ExecutionTimeLimit>
    <Priority>4</Priority>
  </Settings>
  <Actions Context="Author">
    <Exec>
      <Command>{exe}</Command>
      <Arguments>{arguments}</Arguments>
      <WorkingDirectory>{dir}</WorkingDirectory>
    </Exec>
  </Actions>
</Task>
"#
    )
}

/// Extracts the command and arguments from a task definition.
pub fn parse_task(xml: &str) -> Option<TaskInfo> {
    Some(TaskInfo {
        command: xml_text(xml, "Command")?,
        arguments: xml_text(xml, "Arguments").unwrap_or_default(),
    })
}

/// Decodes command output that may be UTF-16LE (with or without BOM) or UTF-8.
fn decode_output(bytes: &[u8]) -> String {
    let (has_bom, bytes) = match bytes.strip_prefix(&[0xFF, 0xFE]) {
        Some(rest) => (true, rest),
        None => (false, bytes),
    };
    // Without a BOM, ASCII text in UTF-16LE still shows a zero in every odd byte
    let looks_utf16 = bytes.len() >= 2 && bytes.iter().skip(1).step_by(2).all(|&b| b == 0);
    if has_bom || looks_utf16 {
        let units: Vec<u16> = bytes
            .as_chunks::<2>()
            .0
            .iter()
            .map(|&c| u16::from_le_bytes(c))
            .collect();
        String::from_utf16_lossy(&units)
    } else {
        String::from_utf8_lossy(bytes).into_owned()
    }
}

/// Compares two Windows paths (case-insensitive, ignoring `/` vs `\`).
pub fn same_path(a: &str, b: &str) -> bool {
    let norm = |s: &str| s.replace('/', "\\").to_lowercase();
    norm(a) == norm(b)
}

/// State of the autostart task as seen from the running exe.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Status {
    NotRegistered,
    /// Registered for this exe
    Registered,
    /// Registered for another exe path (moved or copied): needs re-registration
    Stale(TaskInfo),
}

#[cfg(windows)]
pub use self::windows::*;

#[cfg(windows)]
mod windows {
    use std::os::windows::process::CommandExt;
    use std::process::{Command, Output};
    use std::ptr::null;

    use windows_sys::Win32::Foundation::{CloseHandle, ERROR_CANCELLED, GetLastError};
    use windows_sys::Win32::System::Com::{
        COINIT_APARTMENTTHREADED, COINIT_DISABLE_OLE1DDE, CoInitializeEx,
    };
    use windows_sys::Win32::System::Threading::{
        CREATE_NO_WINDOW, GetExitCodeProcess, INFINITE, WaitForSingleObject,
    };
    use windows_sys::Win32::UI::Shell::{
        SEE_MASK_NOASYNC, SEE_MASK_NOCLOSEPROCESS, SHELLEXECUTEINFOW, ShellExecuteExW,
    };
    use windows_sys::Win32::UI::WindowsAndMessaging::SW_HIDE;

    use super::{Status, TASK_NAME, TaskInfo, decode_output, parse_task, same_path, task_xml};

    /// Runs `schtasks.exe` without flashing a console window.
    fn schtasks(args: &[&str]) -> Result<Output, String> {
        Command::new("schtasks.exe")
            .args(args)
            .creation_flags(CREATE_NO_WINDOW)
            .output()
            .map_err(|e| format!("cannot run schtasks.exe: {e}"))
    }

    /// Turns a failed `schtasks` run into an error message.
    fn check(out: Output) -> Result<Output, String> {
        if out.status.success() {
            Ok(out)
        } else {
            Err(format!(
                "schtasks.exe failed ({}): {}",
                out.status,
                decode_output(&out.stderr).trim()
            ))
        }
    }

    /// Returns the registered task, or None if there is none (or it cannot be read).
    pub fn query() -> Option<TaskInfo> {
        let out = check(schtasks(&["/Query", "/TN", TASK_NAME, "/XML"]).ok()?).ok()?;
        parse_task(&decode_output(&out.stdout))
    }

    /// Path of the running exe.
    pub fn current_exe() -> String {
        std::env::current_exe()
            .map(|p| p.display().to_string())
            .unwrap_or_default()
    }

    /// Returns the autostart state for the running exe.
    pub fn status() -> Status {
        match query() {
            None => Status::NotRegistered,
            Some(task) if same_path(&task.command, &current_exe()) => Status::Registered,
            Some(task) => Status::Stale(task),
        }
    }

    /// The current user as "DOMAIN\user" (for the logon trigger and the principal).
    pub fn current_user() -> String {
        let var = |name| std::env::var(name).unwrap_or_default();
        match var("USERDOMAIN") {
            d if d.is_empty() => var("USERNAME"),
            d => format!("{d}\\{}", var("USERNAME")),
        }
    }

    /// Registers (or replaces) the task to run the current exe with `arguments`.
    /// Needs administrator rights.
    pub fn register(user: &str, arguments: &str) -> Result<(), String> {
        let exe = std::env::current_exe().map_err(|e| format!("cannot get exe path: {e}"))?;
        let dir = exe
            .parent()
            .map(|d| d.display().to_string())
            .unwrap_or_default();
        let xml = task_xml(user, &exe.display().to_string(), arguments, &dir);
        // schtasks reads the definition from a file; UTF-16LE with BOM matches the declared encoding
        let path = std::env::temp_dir().join(format!("nubscroll-task-{}.xml", std::process::id()));
        let bytes: Vec<u8> = [0xFEFFu16]
            .into_iter()
            .chain(xml.encode_utf16())
            .flat_map(u16::to_le_bytes)
            .collect();
        std::fs::write(&path, bytes)
            .map_err(|e| format!("cannot write {}: {e}", path.display()))?;
        let result = schtasks(&[
            "/Create",
            "/TN",
            TASK_NAME,
            "/XML",
            &path.display().to_string(),
            "/F",
        ])
        .and_then(check);
        let _ = std::fs::remove_file(&path);
        result.map(|_| ())
    }

    /// Deletes the task. Needs administrator rights.
    pub fn unregister() -> Result<(), String> {
        check(schtasks(&["/Delete", "/TN", TASK_NAME, "/F"])?).map(|_| ())
    }

    /// Runs the current exe elevated (UAC prompt) with `params` and waits for it.
    /// Returns Ok(true) if it exited with code 0, Ok(false) on another exit code,
    /// Err if it could not be started (including when the user cancelled the prompt).
    pub fn run_elevated(params: &str) -> Result<bool, String> {
        let wide = |s: &str| s.encode_utf16().chain(Some(0)).collect::<Vec<u16>>();
        let (verb, file, params) = (wide("runas"), wide(&current_exe()), wide(params));
        unsafe {
            // ShellExecuteEx may use COM; harmless if already initialized on this thread
            CoInitializeEx(
                null(),
                (COINIT_APARTMENTTHREADED | COINIT_DISABLE_OLE1DDE) as u32,
            );
            let mut info: SHELLEXECUTEINFOW = std::mem::zeroed();
            info.cbSize = std::mem::size_of::<SHELLEXECUTEINFOW>() as u32;
            info.fMask = SEE_MASK_NOCLOSEPROCESS | SEE_MASK_NOASYNC;
            info.lpVerb = verb.as_ptr();
            info.lpFile = file.as_ptr();
            info.lpParameters = params.as_ptr();
            info.nShow = SW_HIDE;
            if ShellExecuteExW(&mut info) == 0 {
                return Err(if GetLastError() == ERROR_CANCELLED {
                    "cancelled at the UAC prompt".into()
                } else {
                    format!("cannot start elevated: {}", std::io::Error::last_os_error())
                });
            }
            if info.hProcess.is_null() {
                return Err("no process handle for the elevated run".into());
            }
            WaitForSingleObject(info.hProcess, INFINITE);
            let mut code = 1u32;
            GetExitCodeProcess(info.hProcess, &mut code);
            CloseHandle(info.hProcess);
            Ok(code == 0)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn quote_arg_follows_windows_rules() {
        assert_eq!(quote_arg("--trace"), "--trace");
        assert_eq!(quote_arg(""), "\"\"");
        assert_eq!(quote_arg("a b"), "\"a b\"");
        assert_eq!(quote_arg(r"C:\My Logs\"), r#""C:\My Logs\\""#);
        assert_eq!(quote_arg(r#"say "hi""#), r#""say \"hi\"""#);
        assert_eq!(quote_arg(r#"a\"b"#), r#""a\\\"b""#);
        // Backslashes not before a quote stay as they are
        assert_eq!(quote_arg(r"C:\a b\c"), r#""C:\a b\c""#);
    }

    #[test]
    fn join_args_quotes_each() {
        let args = ["--log".to_string(), r"C:\a b\x.log".to_string()];
        assert_eq!(join_args(&args), r#"--log "C:\a b\x.log""#);
        assert_eq!(join_args(&[]), "");
    }

    #[test]
    fn task_xml_round_trips_command_and_arguments() {
        let xml = task_xml(
            r"PC\akiva",
            r"C:\Tools & Apps\nubscroll.exe",
            r#"--log "C:\a b\x.log""#,
            r"C:\Tools & Apps",
        );
        assert!(xml.contains("<RunLevel>HighestAvailable</RunLevel>"));
        assert!(xml.contains("<ExecutionTimeLimit>PT0S</ExecutionTimeLimit>"));
        assert!(xml.contains("<StopIfGoingOnBatteries>false</StopIfGoingOnBatteries>"));
        assert!(xml.contains(r"<UserId>PC\akiva</UserId>"));
        assert_eq!(
            parse_task(&xml),
            Some(TaskInfo {
                command: r"C:\Tools & Apps\nubscroll.exe".into(),
                arguments: r#"--log "C:\a b\x.log""#.into(),
            })
        );
    }

    #[test]
    fn parse_task_without_arguments() {
        let xml = "<Exec><Command>C:\\x.exe</Command></Exec>";
        assert_eq!(
            parse_task(xml),
            Some(TaskInfo {
                command: "C:\\x.exe".into(),
                arguments: String::new(),
            })
        );
        assert_eq!(parse_task("<Task/>"), None);
    }

    #[test]
    fn decode_output_handles_utf16_and_utf8() {
        let utf16: Vec<u8> = [0xFEFFu16]
            .into_iter()
            .chain("<Command>C:\\x</Command>".encode_utf16())
            .flat_map(u16::to_le_bytes)
            .collect();
        assert_eq!(decode_output(&utf16), "<Command>C:\\x</Command>");
        assert_eq!(decode_output(b"<Command>"), "<Command>");
    }

    #[test]
    fn same_path_ignores_case_and_slashes() {
        assert!(same_path(
            r"C:\Tools\nubscroll.exe",
            "c:/tools/NUBSCROLL.EXE"
        ));
        assert!(!same_path(r"C:\Tools\nubscroll.exe", r"D:\nubscroll.exe"));
    }
}
