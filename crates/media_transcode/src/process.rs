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
    let display = format!("{command:?}");
    let mut child = command.spawn().map_err(|source| TranscodeError::Spawn {
        command: display.clone(),
        source,
    })?;
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
    let display = format!("{command:?}");
    let mut child = command.spawn().map_err(|source| TranscodeError::Spawn {
        command: display,
        source,
    })?;
    let stdout = child
        .stdout
        .take()
        .ok_or_else(|| TranscodeError::Protocol("child stdout was not piped".into()))?;
    let stderr = child
        .stderr
        .take()
        .ok_or_else(|| TranscodeError::Protocol("child stderr was not piped".into()))?;
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
    let mut eof = false;
    let status = loop {
        if cancelled.load(Ordering::Relaxed) {
            terminate_and_wait(&mut child);
            drop(rx);
            let _ = stdout_thread.join();
            join_reader(stderr_thread)?;
            return Err(TranscodeError::Cancelled);
        }
        while let Ok(item) = rx.try_recv() {
            match item.map_err(TranscodeError::ReadChild)? {
                Some(line) => {
                    if let Err(error) = on_line(&line) {
                        terminate_and_wait(&mut child);
                        drop(rx);
                        let _ = stdout_thread.join();
                        join_reader(stderr_thread)?;
                        return Err(error);
                    }
                }
                None => eof = true,
            }
        }
        if let Some(status) = child.try_wait().map_err(TranscodeError::Wait)? {
            while !eof {
                match rx
                    .recv()
                    .map_err(|_| TranscodeError::Protocol("child stdout reader stopped".into()))?
                    .map_err(TranscodeError::ReadChild)?
                {
                    Some(line) => {
                        if let Err(error) = on_line(&line) {
                            terminate_and_wait(&mut child);
                            drop(rx);
                            let _ = stdout_thread.join();
                            join_reader(stderr_thread)?;
                            return Err(error);
                        }
                    }
                    None => eof = true,
                }
            }
            break status;
        }
        thread::sleep(Duration::from_millis(20));
    };
    stdout_thread
        .join()
        .map_err(|_| TranscodeError::Protocol("child output reader panicked".into()))?;
    let stderr = String::from_utf8_lossy(&join_reader(stderr_thread)?).into_owned();
    Ok((status, stderr))
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
