use std::io;
use windows_sys::Win32::{
    Foundation::{CloseHandle, ERROR_NO_MORE_FILES, HANDLE, INVALID_HANDLE_VALUE},
    System::Diagnostics::ToolHelp::{
        CreateToolhelp32Snapshot, Process32FirstW, Process32NextW, PROCESSENTRY32W,
        TH32CS_SNAPPROCESS,
    },
};

struct Snapshot(HANDLE);

impl Drop for Snapshot {
    fn drop(&mut self) {
        // SAFETY: The wrapper owns the snapshot handle.
        unsafe { CloseHandle(self.0) };
    }
}

pub(crate) fn any_process(mut matches: impl FnMut(&PROCESSENTRY32W) -> bool) -> io::Result<bool> {
    // SAFETY: Tool Help receives a valid snapshot and a correctly sized entry.
    unsafe {
        let snapshot = CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0);
        if snapshot == INVALID_HANDLE_VALUE {
            return Err(io::Error::last_os_error());
        }
        let snapshot = Snapshot(snapshot);
        let mut entry = PROCESSENTRY32W {
            dwSize: std::mem::size_of::<PROCESSENTRY32W>() as u32,
            ..Default::default()
        };
        let mut found = Process32FirstW(snapshot.0, &mut entry);
        while found != 0 {
            if matches(&entry) {
                return Ok(true);
            }
            found = Process32NextW(snapshot.0, &mut entry);
        }
        let error = io::Error::last_os_error();
        if error.raw_os_error() == Some(ERROR_NO_MORE_FILES as i32) {
            Ok(false)
        } else {
            Err(error)
        }
    }
}
