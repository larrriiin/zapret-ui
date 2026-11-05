use std::io;
use std::time::{Duration, Instant};
use windows_sys::Win32::{
    Foundation::ERROR_SERVICE_DOES_NOT_EXIST,
    System::Services::{
        CloseServiceHandle, OpenSCManagerW, OpenServiceW, QueryServiceStatusEx, SC_HANDLE,
        SC_MANAGER_CONNECT, SC_STATUS_PROCESS_INFO, SERVICE_QUERY_STATUS, SERVICE_RUNNING,
        SERVICE_START_PENDING, SERVICE_STATUS_PROCESS, SERVICE_STOPPED, SERVICE_STOP_PENDING,
    },
};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum ServiceState {
    Stopped,
    Starting,
    Stopping,
    Running,
    Other(u32),
}

impl From<u32> for ServiceState {
    fn from(state: u32) -> Self {
        match state {
            SERVICE_STOPPED => Self::Stopped,
            SERVICE_START_PENDING => Self::Starting,
            SERVICE_STOP_PENDING => Self::Stopping,
            SERVICE_RUNNING => Self::Running,
            state => Self::Other(state),
        }
    }
}

struct ServiceHandle(SC_HANDLE);

impl Drop for ServiceHandle {
    fn drop(&mut self) {
        // SAFETY: This wrapper owns one successful SCM/service open operation.
        unsafe { CloseServiceHandle(self.0) };
    }
}

/// Absence is separate from access errors and other failures to read the state.
pub(crate) fn query_state(name: &str) -> io::Result<Option<ServiceState>> {
    let name: Vec<u16> = name.encode_utf16().chain(Some(0)).collect();
    // SAFETY: The service name is NUL-terminated, handles are owned by RAII
    // wrappers, and the output buffer is aligned and sized for this info class.
    unsafe {
        let manager = OpenSCManagerW(std::ptr::null(), std::ptr::null(), SC_MANAGER_CONNECT);
        if manager.is_null() {
            return Err(io::Error::last_os_error());
        }
        let manager = ServiceHandle(manager);
        let service = OpenServiceW(manager.0, name.as_ptr(), SERVICE_QUERY_STATUS);
        if service.is_null() {
            let error = io::Error::last_os_error();
            return if error.raw_os_error() == Some(ERROR_SERVICE_DOES_NOT_EXIST as i32) {
                Ok(None)
            } else {
                Err(error)
            };
        }
        let service = ServiceHandle(service);
        let mut status = SERVICE_STATUS_PROCESS::default();
        let mut needed = 0;
        if QueryServiceStatusEx(
            service.0,
            SC_STATUS_PROCESS_INFO,
            (&mut status as *mut SERVICE_STATUS_PROCESS).cast(),
            std::mem::size_of::<SERVICE_STATUS_PROCESS>() as u32,
            &mut needed,
        ) == 0
        {
            return Err(io::Error::last_os_error());
        }
        Ok(Some(status.dwCurrentState.into()))
    }
}

pub(crate) fn wait_for_stopped(name: &str, timeout: Duration) -> io::Result<()> {
    wait_until_stopped(|| query_state(name), timeout, std::thread::sleep)
}

pub(crate) fn status_label(name: &str) -> &'static str {
    report_state(query_state(name))
}

fn report_state(state: io::Result<Option<ServiceState>>) -> &'static str {
    match state {
        Ok(Some(ServiceState::Running)) => "running",
        Ok(Some(ServiceState::Stopped)) => "stopped",
        Ok(None) => "not_installed",
        _ => "unknown",
    }
}

fn wait_until_stopped(
    mut query: impl FnMut() -> io::Result<Option<ServiceState>>,
    timeout: Duration,
    mut sleep: impl FnMut(Duration),
) -> io::Result<()> {
    let started = Instant::now();
    loop {
        match query()? {
            Some(ServiceState::Stopped) => return Ok(()),
            None => {
                return Err(io::Error::new(
                    io::ErrorKind::NotFound,
                    "Service disappeared",
                ))
            }
            Some(_) => {}
        }
        let remaining = timeout.saturating_sub(started.elapsed());
        if remaining.is_zero() {
            return Err(io::Error::new(
                io::ErrorKind::TimedOut,
                "Service did not reach SERVICE_STOPPED",
            ));
        }
        sleep(remaining.min(Duration::from_millis(100)));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn errors_and_transitions_are_never_reported_as_stopped_or_absent() {
        assert_eq!(
            report_state(Err(io::Error::from_raw_os_error(5))),
            "unknown"
        );
        for state in [
            ServiceState::Starting,
            ServiceState::Stopping,
            ServiceState::Other(7),
        ] {
            assert_eq!(report_state(Ok(Some(state))), "unknown");
        }
        assert_eq!(report_state(Ok(None)), "not_installed");
        assert_eq!(report_state(Ok(Some(ServiceState::Stopped))), "stopped");
        assert_eq!(report_state(Ok(Some(ServiceState::Running))), "running");
    }

    #[test]
    fn stop_pending_is_not_stopped_and_does_not_end_the_wait() {
        let mut states = [
            ServiceState::Running,
            ServiceState::Stopping,
            ServiceState::Stopping,
            ServiceState::Stopped,
        ]
        .into_iter();
        let mut waits = 0;
        wait_until_stopped(
            || Ok(Some(states.next().expect("too many polls"))),
            Duration::from_secs(1),
            |_| waits += 1,
        )
        .unwrap();
        assert_eq!(waits, 3);
    }

    #[test]
    fn timeout_absence_and_query_errors_never_count_as_stopped() {
        assert_eq!(
            wait_until_stopped(
                || Ok(Some(ServiceState::Stopping)),
                Duration::ZERO,
                |_| panic!("must not sleep")
            )
            .unwrap_err()
            .kind(),
            io::ErrorKind::TimedOut
        );
        assert_eq!(
            wait_until_stopped(|| Ok(None), Duration::ZERO, |_| {})
                .unwrap_err()
                .kind(),
            io::ErrorKind::NotFound
        );
        assert_eq!(
            wait_until_stopped(
                || Err(io::Error::from_raw_os_error(5)),
                Duration::ZERO,
                |_| {}
            )
            .unwrap_err()
            .raw_os_error(),
            Some(5)
        );
        wait_until_stopped(|| Ok(Some(ServiceState::Stopped)), Duration::ZERO, |_| {}).unwrap();
    }
}
