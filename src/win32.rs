//! Conversions from Win32 status values to `io::Result`.

use std::io;

use windows_sys::core::{BOOL, HRESULT};

/// For functions that return `FALSE` and set the thread's last error on failure.
pub(crate) fn check_bool(result: BOOL) -> io::Result<()> {
    if result == 0 {
        Err(io::Error::last_os_error())
    } else {
        Ok(())
    }
}

/// For functions that return an `HRESULT`.
pub(crate) fn check_hresult(result: HRESULT) -> io::Result<()> {
    if result < 0 {
        Err(io::Error::from_raw_os_error(result))
    } else {
        Ok(())
    }
}
