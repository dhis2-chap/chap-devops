//! A `docker compose` command with both streams captured and a deadline,
//! for a run that may print much or not stop.

use super::{compose_args, exit_code, spawn_error, trace_command};
use crate::error::Result;
use crate::project::Project;
use std::process::{Command, Stdio};
use std::time::Duration;

/// What a captured compose command did.
#[derive(Debug, Clone)]
pub struct Captured {
    pub code: i32,
    pub stdout: String,
    pub stderr: String,
    /// Whether the deadline passed and the child was killed.
    pub timed_out: bool,
}

impl Captured {
    /// Both streams, in the order a reader would have seen them on a terminal.
    ///
    /// `chapkit test` puts its counts on stdout and its `[FAILED]` lines on
    /// stderr, so anything that reads the run has to read both.
    pub fn text(&self) -> String {
        format!("{}\n{}", self.stdout, self.stderr)
    }
}

/// Run `docker compose <args>` with both streams captured and a deadline.
///
/// Unlike [`compose_output`](super::compose_output) the streams are drained on threads of their own,
/// so this is safe for a command that prints more than a pipe buffer holds,
/// and unlike [`run_compose`](super::run_compose) it can be given up on: `chap-devops models test` has
/// to be able to stop waiting for a model that has wedged. With `echo` the
/// bytes also go to this process's stderr as they arrive, which is what `-v`
/// turns on - stderr rather than stdout, because the caller's own rows are
/// the stdout of the command.
pub fn compose_captured(
    project: &Project,
    extra: &[String],
    echo: bool,
    deadline: Duration,
) -> Result<Captured> {
    let mut args = compose_args(project);
    args.extend(extra.iter().cloned());
    trace_command(&args);

    let mut child = Command::new("docker")
        .args(&args)
        .current_dir(&project.dir)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| spawn_error(&e))?;

    let out = child.stdout.take();
    let err = child.stderr.take();
    let reading_out = std::thread::spawn(move || drain(out, echo));
    let reading_err = std::thread::spawn(move || drain(err, echo));

    let until = std::time::Instant::now() + deadline;
    let mut timed_out = false;
    let status = loop {
        match child.try_wait() {
            Ok(Some(status)) => break status,
            Ok(None) => {}
            Err(e) => return Err(anyhow::anyhow!("waiting for docker: {e}")),
        }
        if std::time::Instant::now() >= until {
            // Only the `exec` client is killed; the process inside the
            // container is the container's own to stop, which is why the
            // caller says what was left behind.
            let _ = child.kill();
            timed_out = true;
            break child
                .wait()
                .map_err(|e| anyhow::anyhow!("waiting for docker: {e}"))?;
        }
        std::thread::sleep(POLL_INTERVAL);
    };

    if timed_out {
        // Not joined: the readers hold pipes the killed child may have handed
        // to a process of its own, and a `models test` that hangs waiting for
        // a model that already hung would be the worse failure. The output of
        // a run that was given up on is not read anyway - under `-v` it has
        // already been echoed - and the threads end when their pipes close.
        return Ok(Captured {
            code: exit_code(status),
            stdout: String::new(),
            stderr: String::new(),
            timed_out,
        });
    }
    Ok(Captured {
        code: exit_code(status),
        stdout: reading_out.join().unwrap_or_default(),
        stderr: reading_err.join().unwrap_or_default(),
        timed_out,
    })
}

/// How often a bounded command is asked whether it has finished.
const POLL_INTERVAL: Duration = Duration::from_millis(100);

/// Read one of a child's streams to the end, echoing it on the way when asked.
fn drain<R: std::io::Read>(stream: Option<R>, echo: bool) -> String {
    use std::io::Write;
    let Some(mut stream) = stream else {
        return String::new();
    };
    let mut raw: Vec<u8> = Vec::new();
    let mut buffer = [0u8; 4096];
    loop {
        match stream.read(&mut buffer) {
            Ok(0) | Err(_) => break,
            Ok(read) => {
                if echo {
                    let mut err = std::io::stderr();
                    let _ = err.write_all(&buffer[..read]);
                    let _ = err.flush();
                }
                raw.extend_from_slice(&buffer[..read]);
            }
        }
    }
    String::from_utf8_lossy(&raw).into_owned()
}
