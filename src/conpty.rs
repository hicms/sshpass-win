//! Pseudo consoles (`ConPTY`) and the processes attached to them.

use std::ffi::c_void;
use std::io::{self, PipeReader, PipeWriter};
use std::marker::PhantomData;
use std::os::windows::io::{AsRawHandle, FromRawHandle, OwnedHandle};
use std::ptr;

use windows_sys::Win32::Foundation::{FALSE, INVALID_HANDLE_VALUE, WAIT_OBJECT_0};
use windows_sys::Win32::System::Console::{
    ClosePseudoConsole, CreatePseudoConsole, HPCON, ResizePseudoConsole,
};
use windows_sys::Win32::System::Threading::{
    CreateProcessW, DeleteProcThreadAttributeList, EXTENDED_STARTUPINFO_PRESENT,
    GetExitCodeProcess, INFINITE, InitializeProcThreadAttributeList, LPPROC_THREAD_ATTRIBUTE_LIST,
    PROC_THREAD_ATTRIBUTE_PSEUDOCONSOLE, PROCESS_INFORMATION, STARTF_USESTDHANDLES, STARTUPINFOEXW,
    TerminateProcess, UpdateProcThreadAttribute, WaitForSingleObject,
};

use crate::cmdline::CommandLine;
use crate::console::Size;
use crate::win32::{check_bool, check_hresult};

/// An open pseudo console; closing it ends the processes attached to it.
#[derive(Debug)]
pub(crate) struct PseudoConsole(HPCON);

impl PseudoConsole {
    /// Creates a pseudo console that reads keystrokes from `input` and
    /// writes rendered output to `output`.
    ///
    /// The console keeps its own duplicates, so the handles passed in are
    /// closed on return: the peer ends then reach end-of-file exactly when
    /// the console goes away.
    pub(crate) fn new(size: Size, input: PipeReader, output: PipeWriter) -> io::Result<Self> {
        let mut handle: HPCON = 0;
        // SAFETY: both pipe handles are open for the duration of the call and
        // `handle` is a valid out-pointer.
        check_hresult(unsafe {
            CreatePseudoConsole(
                size.into(),
                input.as_raw_handle(),
                output.as_raw_handle(),
                0,
                &raw mut handle,
            )
        })?;
        drop((input, output));
        Ok(Self(handle))
    }

    pub(crate) fn resize(&self, size: Size) -> io::Result<()> {
        // SAFETY: `self.0` is a live pseudo console.
        check_hresult(unsafe { ResizePseudoConsole(self.0, size.into()) })
    }

    /// Starts `command_line` attached to this console.
    pub(crate) fn spawn(&self, command_line: &mut CommandLine) -> io::Result<Child> {
        let mut attributes = AttributeList::new(self)?;

        let mut startup = STARTUPINFOEXW::default();
        startup.StartupInfo.cb = STARTUPINFOEXW_SIZE;
        // Without explicitly invalid standard handles, a child can inherit
        // sshpass's own redirected stdio and bypass the pseudo console.
        startup.StartupInfo.dwFlags = STARTF_USESTDHANDLES;
        startup.StartupInfo.hStdInput = INVALID_HANDLE_VALUE;
        startup.StartupInfo.hStdOutput = INVALID_HANDLE_VALUE;
        startup.StartupInfo.hStdError = INVALID_HANDLE_VALUE;
        startup.lpAttributeList = attributes.as_mut_ptr();

        let mut info = PROCESS_INFORMATION::default();
        // SAFETY: `command_line` is a writable NUL-terminated buffer, `startup`
        // is a complete STARTUPINFOEXW whose attribute list outlives the call,
        // and `info` is a valid out-pointer.
        check_bool(unsafe {
            CreateProcessW(
                ptr::null(),
                command_line.as_mut_ptr(),
                ptr::null(),
                ptr::null(),
                FALSE,
                EXTENDED_STARTUPINFO_PRESENT,
                ptr::null(),
                ptr::null(),
                &raw const startup.StartupInfo,
                &raw mut info,
            )
        })?;
        // SAFETY: CreateProcessW succeeded, so both handles are open and ours to close.
        let (process, thread) = unsafe {
            (
                OwnedHandle::from_raw_handle(info.hProcess),
                OwnedHandle::from_raw_handle(info.hThread),
            )
        };
        drop(thread);
        Ok(Child { process })
    }
}

impl Drop for PseudoConsole {
    fn drop(&mut self) {
        // SAFETY: `self.0` is live and is not used after this.
        unsafe { ClosePseudoConsole(self.0) };
    }
}

#[expect(
    clippy::cast_possible_truncation,
    reason = "STARTUPINFOEXW is a small fixed-size struct"
)]
const STARTUPINFOEXW_SIZE: u32 = size_of::<STARTUPINFOEXW>() as u32;

/// A `PROC_THREAD_ATTRIBUTE_LIST` that attaches a new process to a pseudo console.
///
/// The list records the console handle itself, so it borrows the console.
struct AttributeList<'console> {
    /// `usize` elements keep the opaque list pointer-aligned.
    buffer: Box<[usize]>,
    _console: PhantomData<&'console PseudoConsole>,
}

impl<'console> AttributeList<'console> {
    fn new(console: &'console PseudoConsole) -> io::Result<Self> {
        let mut size = 0;
        // SAFETY: with a null list this only reports the required size; the
        // "insufficient buffer" failure it returns is expected.
        unsafe { InitializeProcThreadAttributeList(ptr::null_mut(), 1, 0, &raw mut size) };

        let mut buffer = vec![0_usize; size.div_ceil(size_of::<usize>())].into_boxed_slice();
        // SAFETY: `buffer` is at least `size` bytes and suitably aligned.
        check_bool(unsafe {
            InitializeProcThreadAttributeList(buffer.as_mut_ptr().cast(), 1, 0, &raw mut size)
        })?;
        let mut list = Self {
            buffer,
            _console: PhantomData,
        };

        // SAFETY: the list is initialized with room for one attribute. For this
        // attribute the value is the HPCON itself rather than a pointer to it;
        // the borrow of `console` keeps that handle alive as long as the list.
        check_bool(unsafe {
            UpdateProcThreadAttribute(
                list.as_mut_ptr(),
                0,
                PROC_THREAD_ATTRIBUTE_PSEUDOCONSOLE as usize,
                ptr::without_provenance::<c_void>(console.0.cast_unsigned()),
                size_of::<HPCON>(),
                ptr::null_mut(),
                ptr::null(),
            )
        })?;
        Ok(list)
    }

    fn as_mut_ptr(&mut self) -> LPPROC_THREAD_ATTRIBUTE_LIST {
        self.buffer.as_mut_ptr().cast()
    }
}

impl Drop for AttributeList<'_> {
    fn drop(&mut self) {
        // SAFETY: the list was initialized in `new` and is not used after this.
        unsafe { DeleteProcThreadAttributeList(self.as_mut_ptr()) };
    }
}

/// A process started inside a pseudo console.
#[derive(Debug)]
pub(crate) struct Child {
    process: OwnedHandle,
}

impl Child {
    /// Blocks until the process exits and returns its exit code.
    pub(crate) fn wait(&self) -> io::Result<u32> {
        // SAFETY: the process handle is open.
        if unsafe { WaitForSingleObject(self.process.as_raw_handle(), INFINITE) } != WAIT_OBJECT_0 {
            return Err(io::Error::last_os_error());
        }
        let mut code = 0;
        // SAFETY: the process handle is open and `code` is a valid out-pointer.
        check_bool(unsafe { GetExitCodeProcess(self.process.as_raw_handle(), &raw mut code) })?;
        Ok(code)
    }

    /// Ends the process with `code`; a process that already exited counts as success.
    pub(crate) fn terminate(&self, code: u32) -> io::Result<()> {
        // SAFETY: the process handle is open.
        check_bool(unsafe { TerminateProcess(self.process.as_raw_handle(), code) }).or_else(
            |error| {
                // TerminateProcess reports "access denied" for a process that has already exited.
                // SAFETY: the process handle is open; a zero timeout only polls.
                let exited = unsafe { WaitForSingleObject(self.process.as_raw_handle(), 0) }
                    == WAIT_OBJECT_0;
                if exited { Ok(()) } else { Err(error) }
            },
        )
    }
}
