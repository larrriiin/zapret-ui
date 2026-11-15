use std::ffi::{OsStr, OsString};
use std::io;
use std::os::windows::ffi::OsStrExt;
use windows_sys::Win32::{
    Foundation::{ERROR_FILE_NOT_FOUND, ERROR_PATH_NOT_FOUND, ERROR_SUCCESS},
    System::Registry::{
        RegGetValueW, RegSetKeyValueW, HKEY_CURRENT_USER, REG_DWORD, RRF_RT_REG_DWORD,
    },
};

const ELEVATION_ATTEMPTED_ARG: &str = "--zapret-elevation-attempted";
const PREFERENCES_KEY: windows_sys::core::PCWSTR = windows_sys::w!("Software\\com.alexx.zapret-ui");
const ALWAYS_RUN_AS_ADMIN: windows_sys::core::PCWSTR = windows_sys::w!("AlwaysRunAsAdmin");

fn read_preference() -> io::Result<bool> {
    let mut value = 0u32;
    let mut size = std::mem::size_of_val(&value) as u32;
    // The type filter and buffer size limit the read to one DWORD.
    let status = unsafe {
        RegGetValueW(
            HKEY_CURRENT_USER,
            PREFERENCES_KEY,
            ALWAYS_RUN_AS_ADMIN,
            RRF_RT_REG_DWORD,
            std::ptr::null_mut(),
            (&mut value as *mut u32).cast(),
            &mut size,
        )
    };
    match status {
        ERROR_SUCCESS => Ok(value == 1),
        ERROR_FILE_NOT_FOUND | ERROR_PATH_NOT_FOUND => Ok(false),
        code => Err(io::Error::from_raw_os_error(code as i32)),
    }
}

#[tauri::command]
pub(crate) fn get_always_run_as_admin() -> Result<bool, String> {
    read_preference().map_err(|error| error.to_string())
}

#[tauri::command]
pub(crate) fn set_always_run_as_admin(enabled: bool) -> Result<bool, String> {
    let value = u32::from(enabled);
    // Store the preference for the current Windows account without requesting elevation.
    let status = unsafe {
        RegSetKeyValueW(
            HKEY_CURRENT_USER,
            PREFERENCES_KEY,
            ALWAYS_RUN_AS_ADMIN,
            REG_DWORD,
            (&value as *const u32).cast(),
            std::mem::size_of_val(&value) as u32,
        )
    };
    if status == ERROR_SUCCESS {
        Ok(enabled)
    } else {
        Err(io::Error::from_raw_os_error(status as i32).to_string())
    }
}

pub(crate) fn request_at_startup() -> io::Result<bool> {
    if !read_preference()? {
        return Ok(false);
    }
    request_restart(&std::env::args_os().skip(1).collect::<Vec<_>>())
}

pub(crate) fn request_restart(args: &[OsString]) -> io::Result<bool> {
    elevate_once(crate::is_admin(), args, relaunch_elevated)
}

fn elevate_once(
    is_admin: bool,
    args: &[OsString],
    launch: impl FnOnce(&[u16]) -> io::Result<()>,
) -> io::Result<bool> {
    // The marker is a loop guard, never evidence of administrator privileges.
    if is_admin || args.iter().any(|arg| arg == ELEVATION_ATTEMPTED_ARG) {
        return Ok(false);
    }
    let parameters = elevation_parameters(args)?;
    launch(&parameters)?;
    Ok(true)
}

fn elevation_parameters(args: &[OsString]) -> io::Result<Vec<u16>> {
    let mut parameters = Vec::new();
    for arg in std::iter::once(OsStr::new(ELEVATION_ATTEMPTED_ARG))
        .chain(args.iter().map(OsString::as_os_str))
    {
        if !parameters.is_empty() {
            parameters.push(b' ' as u16);
        }
        // Quote for the Windows argv parser, not for cmd.exe or PowerShell.
        parameters.push(b'"' as u16);
        let mut backslashes = 0;
        for unit in arg.encode_wide() {
            if unit == 0 {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidInput,
                    "NUL in argument",
                ));
            }
            if unit == b'\\' as u16 {
                backslashes += 1;
                continue;
            }
            let count = if unit == b'"' as u16 {
                backslashes * 2 + 1
            } else {
                backslashes
            };
            parameters.extend(std::iter::repeat_n(b'\\' as u16, count));
            parameters.push(unit);
            backslashes = 0;
        }
        parameters.extend(std::iter::repeat_n(b'\\' as u16, backslashes * 2));
        parameters.push(b'"' as u16);
    }
    parameters.push(0);
    Ok(parameters)
}

fn relaunch_elevated(parameters: &[u16]) -> io::Result<()> {
    use windows_sys::Win32::{
        Foundation::{CloseHandle, RPC_E_CHANGED_MODE},
        System::Com::{CoInitializeEx, CoUninitialize, COINIT_APARTMENTTHREADED},
        UI::{
            Shell::{
                ShellExecuteExW, SEE_MASK_FLAG_NO_UI, SEE_MASK_NOASYNC, SEE_MASK_NOCLOSEPROCESS,
                SHELLEXECUTEINFOW,
            },
            WindowsAndMessaging::SW_SHOWNORMAL,
        },
    };

    struct ComGuard(bool);
    impl Drop for ComGuard {
        fn drop(&mut self) {
            if self.0 {
                // SAFETY: Balance the successful CoInitializeEx on this thread.
                unsafe { CoUninitialize() };
            }
        }
    }

    let executable: Vec<u16> = std::env::current_exe()?
        .as_os_str()
        .encode_wide()
        .chain(Some(0))
        .collect();
    let directory: Vec<u16> = std::env::current_dir()?
        .as_os_str()
        .encode_wide()
        .chain(Some(0))
        .collect();
    let verb: Vec<u16> = "runas".encode_utf16().chain(Some(0)).collect();

    // SAFETY: All strings remain alive and NUL-terminated throughout the call.
    // The request is initialized, has the correct size, and owns no input handles.
    unsafe {
        let initialized = CoInitializeEx(std::ptr::null(), COINIT_APARTMENTTHREADED as u32);
        if initialized < 0 && initialized != RPC_E_CHANGED_MODE {
            return Err(io::Error::other(format!(
                "Cannot initialize COM: {initialized:#x}"
            )));
        }
        let _com = ComGuard(initialized >= 0);
        let mut request = SHELLEXECUTEINFOW {
            cbSize: std::mem::size_of::<SHELLEXECUTEINFOW>() as u32,
            fMask: SEE_MASK_NOCLOSEPROCESS | SEE_MASK_NOASYNC | SEE_MASK_FLAG_NO_UI,
            lpVerb: verb.as_ptr(),
            lpFile: executable.as_ptr(),
            lpParameters: parameters.as_ptr(),
            lpDirectory: directory.as_ptr(),
            nShow: SW_SHOWNORMAL,
            ..Default::default()
        };
        if ShellExecuteExW(&mut request) == 0 {
            return Err(io::Error::last_os_error());
        }
        if request.hProcess.is_null() {
            return Err(io::Error::other(
                "Windows did not return the new process handle",
            ));
        }
        CloseHandle(request.hProcess);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::windows::ffi::OsStringExt;

    #[test]
    fn elevated_process_and_marked_false_negative_never_relaunch() {
        for (is_admin, args) in [
            (true, vec![]),
            (false, vec![OsString::from(ELEVATION_ATTEMPTED_ARG)]),
        ] {
            assert!(!elevate_once(is_admin, &args, |_| panic!("must not relaunch")).unwrap());
        }
    }

    #[test]
    fn only_successful_creation_hands_off_to_new_process() {
        let args = [OsString::from("--autostart")];
        assert!(elevate_once(false, &args, |_| Ok(())).unwrap());
        for code in [1223, 2, 5] {
            let result = elevate_once(false, &args, |_| Err(io::Error::from_raw_os_error(code)));
            assert_eq!(result.unwrap_err().raw_os_error(), Some(code));
        }
    }

    fn parse_parameters(parameters: &[u16]) -> Vec<OsString> {
        use windows_sys::Win32::{Foundation::LocalFree, UI::Shell::CommandLineToArgvW};
        let mut command: Vec<u16> = "zapret-ui.exe ".encode_utf16().collect();
        command.extend_from_slice(parameters);
        // SAFETY: CommandLineToArgvW receives a NUL-terminated buffer. Its result
        // contains argc valid pointers and is released using LocalFree.
        unsafe {
            let mut argc = 0;
            let argv = CommandLineToArgvW(command.as_ptr(), &mut argc);
            assert!(!argv.is_null());
            let args = std::slice::from_raw_parts(argv, argc as usize)
                .iter()
                .skip(1)
                .map(|&arg| {
                    let mut len = 0;
                    while *arg.add(len) != 0 {
                        len += 1;
                    }
                    OsString::from_wide(std::slice::from_raw_parts(arg, len))
                })
                .collect();
            LocalFree(argv.cast());
            args
        }
    }

    #[test]
    fn arguments_round_trip_through_windows_parser_with_one_loop_guard() {
        let args = vec![
            OsString::new(),
            OsString::from("--autostart"),
            OsString::from("C:\\Путь с пробелами\\"),
            OsString::from("embedded \"quote\" and \\\"escaped\""),
            OsString::from("apostrophe ' & $ ; ` % !"),
            OsString::from_wide(&[0xd800, b'x' as u16]),
        ];
        let parsed = parse_parameters(&elevation_parameters(&args).unwrap());
        assert_eq!(parsed[0], ELEVATION_ATTEMPTED_ARG);
        assert_eq!(&parsed[1..], args);
        assert!(!elevate_once(false, &parsed, |_| panic!("must not loop")).unwrap());
        assert_eq!(
            parse_parameters(&elevation_parameters(&[]).unwrap()),
            vec![OsString::from(ELEVATION_ATTEMPTED_ARG)]
        );
    }

    #[test]
    fn embedded_nul_is_rejected_before_launch() {
        let args = [OsString::from_wide(&[b'a' as u16, 0, b'b' as u16])];
        let error = elevate_once(false, &args, |_| panic!("must not launch")).unwrap_err();
        assert_eq!(error.kind(), io::ErrorKind::InvalidInput);
    }
}
