use crate::TranscodeError;
use std::{
    io::{self, Read},
    process::{Child, Command, ExitStatus, Stdio},
    sync::{
        atomic::{AtomicBool, Ordering},
        mpsc,
    },
    thread,
    time::Duration,
};

const STDERR_RETAINED_BYTES: usize = 64 * 1024;
const SPAWN_ATTEMPTS: usize = 5;
const SPAWN_RETRY_DELAY: Duration = Duration::from_millis(20);

fn spawn(command: &mut Command, cancelled: &AtomicBool) -> Result<Child, TranscodeError> {
    spawn_with_retry(&format!("{command:?}"), cancelled, || command.spawn())
}

fn spawn_with_retry(
    command: &str,
    cancelled: &AtomicBool,
    attempt: impl FnMut() -> io::Result<Child>,
) -> Result<Child, TranscodeError> {
    spawn_with_retry_budget(command, cancelled, SPAWN_ATTEMPTS, attempt)
}

fn spawn_with_retry_budget(
    command: &str,
    cancelled: &AtomicBool,
    attempts: usize,
    mut attempt: impl FnMut() -> io::Result<Child>,
) -> Result<Child, TranscodeError> {
    // Always make an initial attempt, even if the configured budget is zero.
    let mut retries_remaining = attempts.saturating_sub(1);
    loop {
        if cancelled.load(Ordering::Relaxed) {
            return Err(TranscodeError::Cancelled);
        }
        match attempt() {
            Ok(child) => return Ok(child),
            // On Linux, a concurrent fork can briefly inherit a writable descriptor
            // for a just-created executable, even when that descriptor is CLOEXEC.
            // Retry only failed execs; never rerun a child that actually started.
            Err(source)
                if source.kind() == io::ErrorKind::ExecutableFileBusy && retries_remaining > 0 =>
            {
                retries_remaining -= 1;
                thread::sleep(SPAWN_RETRY_DELAY);
            }
            Err(source) => {
                return Err(TranscodeError::Spawn {
                    command: command.to_owned(),
                    source,
                });
            }
        }
    }
}

pub(crate) struct Output {
    pub status: ExitStatus,
    pub stdout: Vec<u8>,
    pub stderr: String,
}

pub(crate) fn capture(
    command: &mut Command,
    cancelled: &AtomicBool,
) -> Result<Output, TranscodeError> {
    command
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    let mut child = spawn(command, cancelled)?;
    let stdout = child
        .stdout
        .take()
        .ok_or_else(|| TranscodeError::Protocol("child stdout was not piped".into()))?;
    let stderr = child
        .stderr
        .take()
        .ok_or_else(|| TranscodeError::Protocol("child stderr was not piped".into()))?;
    let stdout_thread = thread::spawn(move || read_all(stdout));
    let stderr_thread = thread::spawn(move || read_tail(stderr, STDERR_RETAINED_BYTES));

    let status = loop {
        if cancelled.load(Ordering::Relaxed) {
            terminate_and_wait(&mut child);
            join_reader(stdout_thread)?;
            join_reader(stderr_thread)?;
            return Err(TranscodeError::Cancelled);
        }
        if let Some(status) = child.try_wait().map_err(TranscodeError::Wait)? {
            break status;
        }
        thread::sleep(Duration::from_millis(20));
    };
    Ok(Output {
        status,
        stdout: join_reader(stdout_thread)?,
        stderr: String::from_utf8_lossy(&join_reader(stderr_thread)?).into_owned(),
    })
}

pub(crate) fn lines(
    command: &mut Command,
    cancelled: &AtomicBool,
    mut on_line: impl FnMut(&str) -> Result<(), TranscodeError>,
) -> Result<(ExitStatus, String), TranscodeError> {
    use std::io::BufRead;
    command
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    let mut child = spawn(command, cancelled)?;
    let pipes = (|| {
        let stdout = child
            .stdout
            .take()
            .ok_or_else(|| TranscodeError::Protocol("child stdout was not piped".into()))?;
        let stderr = child
            .stderr
            .take()
            .ok_or_else(|| TranscodeError::Protocol("child stderr was not piped".into()))?;
        Ok::<_, TranscodeError>((stdout, stderr))
    })();
    let (stdout, stderr) = match pipes {
        Ok(pipes) => pipes,
        Err(error) => {
            terminate_and_wait(&mut child);
            return Err(error);
        }
    };
    let (tx, rx) = mpsc::sync_channel::<io::Result<Option<String>>>(32);
    let stdout_thread = thread::spawn(move || {
        let reader = io::BufReader::new(stdout);
        for line in reader.lines() {
            if tx.send(line.map(Some)).is_err() {
                return;
            }
        }
        let _ = tx.send(Ok(None));
    });
    let stderr_thread = thread::spawn(move || read_tail(stderr, STDERR_RETAINED_BYTES));
    let result = (|| {
        let mut eof = false;
        let mut status = None;
        loop {
            if cancelled.load(Ordering::Relaxed) {
                return Err(TranscodeError::Cancelled);
            }
            if !eof {
                match rx.recv_timeout(Duration::from_millis(20)) {
                    Ok(item) => match item.map_err(TranscodeError::ReadChild)? {
                        Some(line) => on_line(&line)?,
                        None => eof = true,
                    },
                    Err(mpsc::RecvTimeoutError::Timeout) => {}
                    Err(mpsc::RecvTimeoutError::Disconnected) => {
                        return Err(TranscodeError::Protocol(
                            "child stdout reader stopped".into(),
                        ));
                    }
                }
            } else if status.is_none() {
                // EOF disconnects the channel; do not busy-spin while the child exits.
                thread::sleep(Duration::from_millis(20));
            }
            if status.is_none() {
                status = child.try_wait().map_err(TranscodeError::Wait)?;
            }
            if eof {
                if let Some(status) = status {
                    return Ok(status);
                }
            }
        }
    })();
    if result.is_err() {
        terminate_and_wait(&mut child);
    }
    // Disconnect before joining: a failed callback may leave the bounded sender blocked.
    drop(rx);
    let stdout_result = stdout_thread
        .join()
        .map_err(|_| TranscodeError::Protocol("child output reader panicked".into()));
    let stderr_result = join_reader(stderr_thread);
    // Join both readers even on failure, preserving the original operation error.
    let status = result?;
    stdout_result?;
    let stderr = String::from_utf8_lossy(&stderr_result?).into_owned();
    Ok((status, stderr))
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;

    fn shell(script: &str) -> Command {
        let mut command = Command::new("/bin/sh");
        command.args(["-c", script]);
        command
    }

    #[test]
    fn spawn_retries_only_transient_executable_contention() {
        let mut attempts = 0;
        let mut child = spawn_with_retry("mock", &AtomicBool::new(false), || {
            attempts += 1;
            if attempts == 1 {
                Err(io::ErrorKind::ExecutableFileBusy.into())
            } else {
                shell("exit 0").spawn()
            }
        })
        .unwrap();
        assert!(child.wait().unwrap().success());
        assert_eq!(attempts, 2);
    }

    #[test]
    fn spawn_persistent_contention_preserves_error_and_stops() {
        let mut attempts = 0;
        let error = spawn_with_retry("mock", &AtomicBool::new(false), || {
            attempts += 1;
            Err(io::ErrorKind::ExecutableFileBusy.into())
        })
        .unwrap_err();
        assert_eq!(attempts, SPAWN_ATTEMPTS);
        assert!(matches!(error, TranscodeError::Spawn { command, source }
            if command == "mock" && source.kind() == io::ErrorKind::ExecutableFileBusy));
    }

    #[test]
    fn spawn_zero_one_and_default_budgets_preserve_terminal_error() {
        for (budget, expected_attempts) in [(0, 1), (1, 1), (SPAWN_ATTEMPTS, 5)] {
            let mut attempts = 0;
            let error = spawn_with_retry_budget("mock", &AtomicBool::new(false), budget, || {
                attempts += 1;
                Err(io::Error::new(
                    io::ErrorKind::ExecutableFileBusy,
                    "original spawn failure",
                ))
            })
            .unwrap_err();
            assert_eq!(attempts, expected_attempts);
            assert!(matches!(error, TranscodeError::Spawn { command, source }
                if command == "mock"
                    && source.kind() == io::ErrorKind::ExecutableFileBusy
                    && source.to_string() == "original spawn failure"));
        }
    }

    #[test]
    fn spawn_cancelled_zero_budget_does_not_attempt() {
        let error = spawn_with_retry_budget("mock", &AtomicBool::new(true), 0, || {
            panic!("cancelled spawn must not be attempted")
        })
        .unwrap_err();
        assert!(matches!(error, TranscodeError::Cancelled));
    }

    #[test]
    fn spawn_cancellation_stops_contention_retry() {
        let cancelled = AtomicBool::new(false);
        let mut attempts = 0;
        let error = spawn_with_retry("mock", &cancelled, || {
            attempts += 1;
            cancelled.store(true, Ordering::Relaxed);
            Err(io::ErrorKind::ExecutableFileBusy.into())
        })
        .unwrap_err();
        assert!(matches!(error, TranscodeError::Cancelled));
        assert_eq!(attempts, 1);
    }

    #[test]
    fn spawn_other_errors_are_not_retried() {
        let mut attempts = 0;
        let error = spawn_with_retry("mock", &AtomicBool::new(false), || {
            attempts += 1;
            Err(io::ErrorKind::PermissionDenied.into())
        })
        .unwrap_err();
        assert_eq!(attempts, 1);
        assert!(matches!(error, TranscodeError::Spawn { source, .. }
            if source.kind() == io::ErrorKind::PermissionDenied));
    }

    #[test]
    fn cancelled_capture_and_lines_do_not_spawn() {
        let cancelled = AtomicBool::new(true);
        let mut command = Command::new("/nonexistent-media-transcode-tool");
        assert!(matches!(
            capture(&mut command, &cancelled),
            Err(TranscodeError::Cancelled)
        ));
        assert!(matches!(
            lines(&mut command, &cancelled, |_| panic!("must not start")),
            Err(TranscodeError::Cancelled)
        ));
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn spawn_recovers_from_real_etxtbsy_after_writer_closes() {
        use std::{fs, os::unix::fs::PermissionsExt};

        let directory = tempfile::tempdir().unwrap();
        let executable = directory.path().join("mock.sh");
        fs::write(&executable, "#!/bin/sh\nexit 0\n").unwrap();
        fs::set_permissions(&executable, fs::Permissions::from_mode(0o755)).unwrap();
        let mut writer = Some(
            fs::OpenOptions::new()
                .write(true)
                .open(&executable)
                .unwrap(),
        );
        let mut command = Command::new(&executable);
        let mut attempts = 0;
        let mut child = spawn_with_retry("mock", &AtomicBool::new(false), || {
            attempts += 1;
            let result = command.spawn();
            if attempts == 1 {
                assert_eq!(
                    result.as_ref().unwrap_err().raw_os_error(),
                    Some(libc::ETXTBSY)
                );
                // Release only after observing the real error, not after a timed sleep.
                drop(writer.take());
            }
            result
        })
        .unwrap();
        assert!(child.wait().unwrap().success());
        assert_eq!(attempts, 2);
    }

    fn assert_reaped(pid: &str) {
        // kill -0 also sees unreaped zombies, so this checks termination and reaping.
        let status = Command::new("/bin/sh")
            .args(["-c", "kill -0 \"$1\" 2>/dev/null", "check", pid])
            .status()
            .unwrap();
        assert!(
            !status.success(),
            "owned child {pid} remains alive or unreaped"
        );
    }

    #[test]
    fn lines_preserves_large_burst_order_eof_and_nonzero_status() {
        let mut command = shell(
            "i=0; while [ $i -lt 128 ]; do echo $i; i=$((i + 1)); done; printf final; printf diagnostic >&2; exit 7",
        );
        let mut output = Vec::new();
        let (status, stderr) = lines(&mut command, &AtomicBool::new(false), |line| {
            output.push(line.to_owned());
            Ok(())
        })
        .unwrap();
        let mut expected: Vec<_> = (0..128).map(|i| i.to_string()).collect();
        expected.push("final".into());
        assert_eq!(output, expected);
        assert_eq!(status.code(), Some(7));
        assert_eq!(stderr, "diagnostic");
    }

    #[test]
    fn lines_cancellation_during_burst_reaps_child() {
        let mut command = shell(
            "echo $$; i=0; while [ $i -lt 128 ]; do echo line; i=$((i + 1)); done; exec sleep 5",
        );
        let cancelled = AtomicBool::new(false);
        let mut pid = String::new();
        let mut callbacks = 0;
        let result = lines(&mut command, &cancelled, |line| {
            pid = line.to_owned();
            callbacks += 1;
            cancelled.store(true, Ordering::Relaxed);
            Ok(())
        });
        assert!(matches!(result, Err(TranscodeError::Cancelled)));
        assert_eq!(callbacks, 1, "cancellation must be checked between lines");
        assert_reaped(&pid);
    }

    #[test]
    fn lines_callback_failure_reaps_child_during_burst() {
        let mut command = shell(
            "echo $$; i=0; while [ $i -lt 128 ]; do echo line; i=$((i + 1)); done; exec sleep 5",
        );
        let mut pid = String::new();
        let result = lines(&mut command, &AtomicBool::new(false), |line| {
            pid = line.to_owned();
            Err(TranscodeError::Protocol("callback failed".into()))
        });
        assert!(
            matches!(result, Err(TranscodeError::Protocol(message)) if message == "callback failed")
        );
        assert_reaped(&pid);
    }

    #[test]
    fn lines_invalid_utf8_reaps_child() {
        let mut command = shell("echo $$; printf '\\377\\n'; exec sleep 5");
        let mut pid = String::new();
        let result = lines(&mut command, &AtomicBool::new(false), |line| {
            pid = line.to_owned();
            Ok(())
        });
        assert!(
            matches!(result, Err(TranscodeError::ReadChild(error)) if error.kind() == io::ErrorKind::InvalidData)
        );
        assert_reaped(&pid);
    }

    #[test]
    fn lines_retains_bounded_stderr_tail() {
        let mut command =
            shell("i=0; while [ $i -lt 8193 ]; do printf 01234567 >&2; i=$((i + 1)); done");
        let (status, stderr) = lines(&mut command, &AtomicBool::new(false), |_| Ok(())).unwrap();
        assert!(status.success());
        assert_eq!(stderr, "01234567".repeat(STDERR_RETAINED_BYTES / 8));
    }

    #[test]
    fn lines_waits_for_exit_after_stdout_eof() {
        let mut command = shell("exec 1>&-; printf tail >&2; exit 9");
        let (status, stderr) = lines(&mut command, &AtomicBool::new(false), |_| {
            panic!("closed stdout must not produce lines")
        })
        .unwrap();
        assert_eq!(status.code(), Some(9));
        assert_eq!(stderr, "tail");
    }
}

fn terminate_and_wait(child: &mut Child) {
    let _ = child.kill();
    let _ = child.wait();
}

fn read_all(mut reader: impl Read) -> io::Result<Vec<u8>> {
    let mut bytes = Vec::new();
    reader.read_to_end(&mut bytes)?;
    Ok(bytes)
}

fn read_tail(mut reader: impl Read, retained: usize) -> io::Result<Vec<u8>> {
    let mut tail = Vec::with_capacity(retained);
    let mut buffer = [0_u8; 8192];
    loop {
        let count = reader.read(&mut buffer)?;
        if count == 0 {
            return Ok(tail);
        }
        if count >= retained {
            tail.clear();
            tail.extend_from_slice(&buffer[count - retained..count]);
        } else {
            let overflow = tail.len().saturating_add(count).saturating_sub(retained);
            if overflow > 0 {
                tail.drain(..overflow);
            }
            tail.extend_from_slice(&buffer[..count]);
        }
    }
}

fn join_reader(handle: thread::JoinHandle<io::Result<Vec<u8>>>) -> Result<Vec<u8>, TranscodeError> {
    handle
        .join()
        .map_err(|_| TranscodeError::Protocol("child output reader panicked".into()))?
        .map_err(TranscodeError::ReadChild)
}
