//! Scroll cursor: shows an arrow cursor for the scroll axis while scrolling (Windows only).
//!
//! There is no API to change the cursor of other processes' windows, so the system cursors
//! (arrow, I-beam, hand, ...) are temporarily replaced with `SetSystemCursor`, as other wheel
//! tools do. They are restored by reloading the user's cursor scheme (`SPI_SETCURSORS`),
//! which also repairs cursors left behind if a previous run was killed while scrolling.

use std::ptr::null_mut;
use std::sync::mpsc::{self, RecvTimeoutError, Sender};
use std::thread;
use std::time::{Duration, Instant};

use windows_sys::Win32::Foundation::HANDLE;
use windows_sys::Win32::UI::WindowsAndMessaging::{
    CopyIcon, DestroyCursor, HCURSOR, IDC_SIZEALL, IDC_SIZENS, IDC_SIZEWE, LoadCursorW,
    OCR_APPSTARTING, OCR_CROSS, OCR_HAND, OCR_IBEAM, OCR_NO, OCR_NORMAL, OCR_SIZEALL, OCR_SIZENESW,
    OCR_SIZENS, OCR_SIZENWSE, OCR_SIZEWE, OCR_UP, OCR_WAIT, SPI_SETCURSORS, SYSTEM_CURSOR_ID,
    SetSystemCursor, SystemParametersInfoW,
};

use crate::gesture::Axes;
use crate::logger::Logger;

/// System cursors replaced while scrolling (the ones apps commonly show).
const REPLACED: [SYSTEM_CURSOR_ID; 13] = [
    OCR_NORMAL,
    OCR_IBEAM,
    OCR_WAIT,
    OCR_CROSS,
    OCR_UP,
    OCR_SIZENWSE,
    OCR_SIZENESW,
    OCR_SIZEWE,
    OCR_SIZENS,
    OCR_SIZEALL,
    OCR_NO,
    OCR_HAND,
    OCR_APPSTARTING,
];

/// What the cursor should show, sent to the cursor thread whenever it changes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Indicator {
    /// Normal cursors
    Off,
    /// The middle button is held but not moved yet: show the four-way arrow after the hold delay
    /// (a quick middle click then never flashes the cursor)
    Hold,
    /// Scrolling along the given axes
    Scroll(Axes),
}

/// Restores the user's system cursors. Safe to call at any time and from any thread.
pub fn restore() -> bool {
    unsafe { SystemParametersInfoW(SPI_SETCURSORS, 0, null_mut(), 0) != 0 }
}

/// Starts the cursor thread and returns the sending end of its queue.
/// `hold_delay` is how long the middle button must be held before [`Indicator::Hold`] shows.
pub fn start(log: Logger, hold_delay: Duration) -> std::io::Result<Sender<Indicator>> {
    let (tx, rx) = mpsc::channel::<Indicator>();
    thread::Builder::new()
        .name("cursor".into())
        .spawn(move || {
            // Repair cursors left behind by a previous run before taking copies of the originals
            restore();
            let arrows = Arrows::load();
            // Whether the system cursors are currently replaced (skip needless restores on clicks)
            let mut replaced = false;
            // When a pending Hold should be shown
            let mut hold_at: Option<Instant> = None;
            loop {
                // Wait for the next request, or until a pending Hold is due
                let request = match hold_at {
                    None => rx.recv().map_err(|_| RecvTimeoutError::Disconnected),
                    Some(at) => rx.recv_timeout(at.saturating_duration_since(Instant::now())),
                };
                let target = match request {
                    Ok(Indicator::Hold) => {
                        hold_at = Some(Instant::now() + hold_delay);
                        continue;
                    }
                    Ok(Indicator::Scroll(axes)) => Some(axes),
                    Ok(Indicator::Off) => None,
                    // The button is still held after the delay: show the four-way arrow
                    Err(RecvTimeoutError::Timeout) => Some(Axes::Both),
                    Err(RecvTimeoutError::Disconnected) => break,
                };
                hold_at = None;
                let ok = match target {
                    Some(axes) => {
                        replaced = true;
                        arrows.show(axes)
                    }
                    None if replaced => {
                        replaced = false;
                        restore()
                    }
                    None => true,
                };
                if !ok {
                    log.log(format!(
                        "[warn] cannot change cursor to {target:?} (error={})",
                        std::io::Error::last_os_error()
                    ));
                }
            }
            restore();
        })?;
    Ok(tx)
}

/// Private copies of the arrow cursors. Loaded once before any replacement: once a system cursor
/// is replaced, `LoadCursorW` returns the replacement, not the original image.
struct Arrows {
    vertical: HCURSOR,
    horizontal: HCURSOR,
    both: HCURSOR,
}

impl Arrows {
    /// Copies the standard arrow cursors.
    fn load() -> Self {
        let copy = |id| unsafe { CopyIcon(LoadCursorW(null_mut(), id)) };
        Self {
            vertical: copy(IDC_SIZENS),
            horizontal: copy(IDC_SIZEWE),
            both: copy(IDC_SIZEALL),
        }
    }

    /// Replaces all system cursors in [`REPLACED`] with the arrow for `axes`.
    fn show(&self, axes: Axes) -> bool {
        let source = match axes {
            Axes::Vertical => self.vertical,
            Axes::Horizontal => self.horizontal,
            Axes::Both => self.both,
        };
        if source.is_null() {
            return false;
        }
        REPLACED.iter().all(|&id| unsafe {
            // SetSystemCursor takes ownership of (and destroys) the handle, so pass a fresh copy each time
            let copy: HANDLE = CopyIcon(source);
            !copy.is_null() && SetSystemCursor(copy, id) != 0
        })
    }
}

impl Drop for Arrows {
    fn drop(&mut self) {
        for c in [self.vertical, self.horizontal, self.both] {
            if !c.is_null() {
                unsafe { DestroyCursor(c) };
            }
        }
    }
}
