//! Running from an MSIX package (the Microsoft Store), and what changes then (Windows only).
//!
//! The same exe runs from the zip and the installer too. Packaged, Windows redirects the app's
//! writes to `%APPDATA%` and `HKCU` into a private copy that only the package sees, and the exe's
//! path changes with every update. So, packaged, the settings file goes to the package's own
//! folder (`LocalState`, a real folder every program can open) and starting with Windows is the
//! startup task declared in the package's manifest, not a scheduled task (which could not be
//! elevated from a package anyway).

use std::path::PathBuf;
use std::sync::OnceLock;

use windows_sys::Win32::Foundation::{APPMODEL_ERROR_NO_PACKAGE, ERROR_INSUFFICIENT_BUFFER};
use windows_sys::Win32::Storage::Packaging::Appx::GetCurrentPackageFamilyName;

/// The package family name (e.g. `C2B504BA.nubmiddle_1raghr5p0ryyw`), if running from a package.
/// Looked up once: it cannot change while running.
pub fn family_name() -> Option<&'static str> {
    static NAME: OnceLock<Option<String>> = OnceLock::new();
    NAME.get_or_init(query_family_name).as_deref()
}

/// Whether running from an MSIX package.
pub fn is_packaged() -> bool {
    family_name().is_some()
}

/// The package's folder for its own files, `%LOCALAPPDATA%\Packages\<family name>\LocalState`,
/// if packaged.
pub fn local_state() -> Option<PathBuf> {
    let family = family_name()?;
    let local = std::env::var_os("LOCALAPPDATA")?;
    Some(
        PathBuf::from(local)
            .join("Packages")
            .join(family)
            .join("LocalState"),
    )
}

/// Asks Windows for the family name of the package this process runs in.
fn query_family_name() -> Option<String> {
    let mut length = 0u32;
    unsafe {
        // A size query: Windows writes the length needed (counting the terminating zero)
        let result = GetCurrentPackageFamilyName(&mut length, std::ptr::null_mut());
        if result == APPMODEL_ERROR_NO_PACKAGE || result != ERROR_INSUFFICIENT_BUFFER || length == 0
        {
            return None;
        }
        let mut buffer = vec![0u16; length as usize];
        if GetCurrentPackageFamilyName(&mut length, buffer.as_mut_ptr()) != 0 {
            return None;
        }
        Some(String::from_utf16_lossy(
            &buffer[..(length as usize).saturating_sub(1)],
        ))
    }
}

/// The package's startup task ("Start with Windows" of the Store version).
pub mod startup_task {
    use windows::ApplicationModel::{StartupTask, StartupTaskState};
    use windows::core::HSTRING;

    /// `TaskId` of the `windows.startupTask` extension in `packaging/msix/AppxManifest.xml`.
    const TASK_ID: &str = "nubmiddle";

    /// Looks up the task.
    fn task() -> Result<StartupTask, String> {
        StartupTask::GetAsync(&HSTRING::from(TASK_ID))
            .and_then(|op| op.join())
            .map_err(|e| format!("cannot read the startup task: {e}"))
    }

    /// Whether the task is on.
    pub fn is_enabled() -> Result<bool, String> {
        let state = task()?
            .State()
            .map_err(|e| format!("cannot read the startup task state: {e}"))?;
        Ok(matches!(
            state,
            StartupTaskState::Enabled | StartupTaskState::EnabledByPolicy
        ))
    }

    /// Turns the task on or off. Fails when the user turned it off in Windows (Task Manager or
    /// Settings > Apps > Startup): only they can turn it on again there.
    pub fn set(enabled: bool) -> Result<(), String> {
        let task = task()?;
        if !enabled {
            return task
                .Disable()
                .map_err(|e| format!("cannot turn the startup task off: {e}"));
        }
        let state = task
            .RequestEnableAsync()
            .and_then(|op| op.join())
            .map_err(|e| format!("cannot turn the startup task on: {e}"))?;
        match state {
            StartupTaskState::Enabled | StartupTaskState::EnabledByPolicy => Ok(()),
            StartupTaskState::DisabledByUser => Err(
                "starting with Windows was turned off in Windows Settings > Apps > Startup; turn it on there"
                    .into(),
            ),
            other => Err(format!("the startup task stayed off (state {})", other.0)),
        }
    }
}
