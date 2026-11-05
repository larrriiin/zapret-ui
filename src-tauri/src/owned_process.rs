use std::io;
use std::process::Child;

/// Stop exactly the child represented by this handle. Never fall back to an
/// image-name search, and never block waiting when termination has failed.
pub(crate) fn stop(child: &mut Child) -> io::Result<()> {
    if child.try_wait()?.is_some() {
        return Ok(());
    }
    if let Err(error) = child.kill() {
        // The process may have exited between try_wait and kill.
        return if child.try_wait()?.is_some() {
            Ok(())
        } else {
            Err(error)
        };
    }
    child.wait()?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::process::{Command, Stdio};

    const FIXTURE_ENV: &str = "ZAPRET_OWNED_PROCESS_TEST_CHILD";

    #[test]
    fn sleeping_child_fixture() {
        if std::env::var_os(FIXTURE_ENV).is_some() {
            std::thread::sleep(std::time::Duration::from_secs(30));
        }
    }

    struct Fixture(Child);
    impl Fixture {
        fn spawn() -> Self {
            let mut command = Command::new(std::env::current_exe().unwrap());
            command
                .args(["--exact", "owned_process::tests::sleeping_child_fixture"])
                .env(FIXTURE_ENV, "1")
                .stdin(Stdio::null())
                .stdout(Stdio::null())
                .stderr(Stdio::null());
            #[cfg(windows)]
            {
                use std::os::windows::process::CommandExt;
                command.creation_flags(0x08000000);
            }
            Self(command.spawn().unwrap())
        }
    }
    impl Drop for Fixture {
        fn drop(&mut self) {
            let _ = stop(&mut self.0);
        }
    }

    #[test]
    fn stopping_one_child_preserves_another_process_with_the_same_image() {
        let mut owned = Fixture::spawn();
        let mut other = Fixture::spawn();
        assert!(other.0.try_wait().unwrap().is_none());
        stop(&mut owned.0).unwrap();
        assert!(owned.0.try_wait().unwrap().is_some());
        assert!(other.0.try_wait().unwrap().is_none());
        // Stopping an already exited child is harmless.
        stop(&mut owned.0).unwrap();
    }
}
