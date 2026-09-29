//! The user's console: modes, window size and raw input.

use std::io;
use std::os::windows::io::{AsRawHandle, BorrowedHandle};
use std::ptr;

use windows_sys::Win32::System::Console::{
    CONSOLE_MODE, CONSOLE_SCREEN_BUFFER_INFO, COORD, GetConsoleMode, GetConsoleScreenBufferInfo,
    ReadConsoleW, SetConsoleMode,
};

use crate::win32::check_bool;

/// Size of a console window in character cells.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Size {
    columns: i16,
    rows: i16,
}

impl Size {
    /// Used when stdout is not a console.
    pub(crate) const FALLBACK: Self = Self {
        columns: 120,
        rows: 25,
    };
}

impl From<Size> for COORD {
    fn from(size: Size) -> Self {
        Self {
            X: size.columns,
            Y: size.rows,
        }
    }
}

/// Puts a console handle back into its original mode when dropped.
#[derive(Debug)]
pub(crate) struct ModeGuard<'a> {
    handle: BorrowedHandle<'a>,
    original: CONSOLE_MODE,
}

impl<'a> ModeGuard<'a> {
    /// Applies `change` to the mode of `handle`; `Ok(None)` means the handle
    /// is not a console (for example, redirected to a file or pipe).
    pub(crate) fn update(
        handle: BorrowedHandle<'a>,
        change: impl FnOnce(CONSOLE_MODE) -> CONSOLE_MODE,
    ) -> io::Result<Option<Self>> {
        let Some(original) = mode(handle) else {
            return Ok(None);
        };
        set_mode(handle, change(original))?;
        Ok(Some(Self { handle, original }))
    }
}

impl Drop for ModeGuard<'_> {
    fn drop(&mut self) {
        // Nothing useful can be done about a failure during teardown.
        let _ = set_mode(self.handle, self.original);
    }
}

fn mode(handle: BorrowedHandle<'_>) -> Option<CONSOLE_MODE> {
    let mut mode = 0;
    // SAFETY: `handle` is open for the borrow and `mode` is a valid out-pointer.
    let ok = unsafe { GetConsoleMode(handle.as_raw_handle(), &raw mut mode) } != 0;
    ok.then_some(mode)
}

fn set_mode(handle: BorrowedHandle<'_>, mode: CONSOLE_MODE) -> io::Result<()> {
    // SAFETY: `handle` is open for the borrow.
    check_bool(unsafe { SetConsoleMode(handle.as_raw_handle(), mode) })
}

/// The visible window size, or `None` if `handle` is not a console screen buffer.
pub(crate) fn window_size(handle: BorrowedHandle<'_>) -> Option<Size> {
    let mut info = CONSOLE_SCREEN_BUFFER_INFO::default();
    // SAFETY: `handle` is open for the borrow and `info` is a valid out-pointer.
    let ok = unsafe { GetConsoleScreenBufferInfo(handle.as_raw_handle(), &raw mut info) } != 0;
    let window = info.srWindow;
    ok.then(|| Size {
        columns: window.Right - window.Left + 1,
        rows: window.Bottom - window.Top + 1,
    })
}

/// Reads console input as UTF-16.
///
/// std's console reader treats Ctrl+Z as end of input; here it is an
/// ordinary keystroke that belongs to the remote side.
pub(crate) fn read_utf16(handle: BorrowedHandle<'_>, buffer: &mut [u16]) -> io::Result<usize> {
    let capacity = u32::try_from(buffer.len()).unwrap_or(u32::MAX);
    let mut read = 0;
    // SAFETY: `buffer` is valid for `capacity` UTF-16 units, `read` is a valid
    // out-pointer, and a null input control is allowed.
    check_bool(unsafe {
        ReadConsoleW(
            handle.as_raw_handle(),
            buffer.as_mut_ptr().cast(),
            capacity,
            &raw mut read,
            ptr::null(),
        )
    })?;
    Ok(read as usize)
}
