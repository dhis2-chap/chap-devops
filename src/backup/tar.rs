//! Building and reading the archive with the system `tar` binary.

use crate::error::Result;
use std::fs::File;
use std::io::Read;
use std::path::Path;
use std::process::{Command, Stdio};

/// `tar`, configured the same way for every call.
fn tar_command() -> Command {
    let mut cmd = Command::new("tar");
    // Without this macOS tar stores an AppleDouble `._name` member beside every
    // file that carries extended attributes, which nothing here wants.
    cmd.env("COPYFILE_DISABLE", "1");
    cmd.stdin(Stdio::null());
    cmd
}

/// Pack `members` of `stage` into `archive` as gzipped tar.
///
/// The archive holds `.env` and the database dumps, so it is created
/// readable by its owner only, as `.env` is. tar writes to stdout because
/// it would create a file of its own with the default mode.
pub fn tar_create(archive: &Path, stage: &Path, members: &[String]) -> Result<()> {
    let file = owner_only_file(archive)?;
    let mut cmd = tar_command();
    cmd.arg("-czf").arg("-").arg("-C").arg(stage);
    cmd.args(members);
    cmd.stdout(Stdio::from(file));
    finish_tar(cmd, &format!("writing {}", archive.display()))?;
    Ok(())
}

/// A new, empty file at `path` that only its owner can read and write.
fn owner_only_file(path: &Path) -> Result<File> {
    let _ = std::fs::remove_file(path);
    let mut options = std::fs::OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    options
        .open(path)
        .map_err(|e| anyhow::anyhow!("creating {}: {e}", path.display()))
}

/// The member names inside `archive`.
pub fn tar_list(archive: &Path) -> Result<Vec<String>> {
    let mut cmd = tar_command();
    cmd.arg("-tzf").arg(archive);
    let out = finish_tar(cmd, &format!("reading {}", archive.display()))?;
    Ok(out
        .lines()
        .map(|l| l.trim_end_matches('/').to_string())
        .filter(|l| !l.is_empty())
        .collect())
}

/// One member's bytes as text, without unpacking the archive.
pub fn tar_read_member(archive: &Path, member: &str) -> Result<String> {
    let mut cmd = tar_command();
    cmd.arg("-xzf").arg(archive).arg("-O").arg(member);
    finish_tar(cmd, &format!("reading {member} from {}", archive.display()))
}

/// Unpack one member of `archive` to `dest`, keeping the member's own name out
/// of it: the file lands at `dest` exactly.
pub fn tar_extract_member_to(archive: &Path, member: &str, dest: &Path) -> Result<()> {
    let file =
        File::create(dest).map_err(|e| anyhow::anyhow!("creating {}: {e}", dest.display()))?;
    let mut cmd = tar_command();
    cmd.arg("-xzf")
        .arg(archive)
        .arg("-O")
        .arg(member)
        .stdout(Stdio::from(file))
        .stderr(Stdio::piped());
    let mut child = cmd.spawn().map_err(|e| tar_spawn_error(&e))?;
    let stderr = read_all(child.stderr.take());
    let status = child
        .wait()
        .map_err(|e| anyhow::anyhow!("waiting for tar: {e}"))?;
    if !status.success() {
        return Err(anyhow::anyhow!(
            "extracting {member} from {}: {}",
            archive.display(),
            first_line(&stderr)
        ));
    }
    Ok(())
}

/// Unpack `members` of `archive` into `dest`, keeping their paths.
pub fn tar_extract_into(archive: &Path, dest: &Path, members: &[String]) -> Result<()> {
    std::fs::create_dir_all(dest)
        .map_err(|e| anyhow::anyhow!("creating {}: {e}", dest.display()))?;
    let mut cmd = tar_command();
    cmd.arg("-xzf").arg(archive).arg("-C").arg(dest);
    cmd.args(members);
    finish_tar(cmd, &format!("unpacking {}", archive.display()))?;
    Ok(())
}

fn finish_tar(mut cmd: Command, what: &str) -> Result<String> {
    let out = cmd.output().map_err(|e| tar_spawn_error(&e))?;
    if !out.status.success() {
        return Err(anyhow::anyhow!(
            "{what}: tar failed: {}",
            first_line(&String::from_utf8_lossy(&out.stderr))
        ));
    }
    Ok(String::from_utf8_lossy(&out.stdout).into_owned())
}

fn tar_spawn_error(err: &std::io::Error) -> anyhow::Error {
    if err.kind() == std::io::ErrorKind::NotFound {
        anyhow::anyhow!(
            "`tar` was not found on PATH; chap-devops builds and reads backup archives with it"
        )
    } else {
        anyhow::anyhow!("could not run `tar`: {err}")
    }
}

pub(super) fn read_all(stream: Option<impl Read>) -> String {
    let mut text = String::new();
    if let Some(mut stream) = stream {
        let _ = stream.read_to_string(&mut text);
    }
    text
}

/// The first non-empty line of a child's stderr, for a one-line error.
pub fn first_line(text: &str) -> String {
    text.lines()
        .map(str::trim)
        .find(|l| !l.is_empty())
        .unwrap_or("no output")
        .to_string()
}
