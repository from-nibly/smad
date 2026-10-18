//! Exact Git objects, without a shell, external diff drivers, filters or textconv.
use crate::source::{MAX_SOURCE_BYTES, SourceText};
use std::{
    io::Read,
    os::fd::AsRawFd,
    path::{Path, PathBuf},
    process::{Child, Command, Stdio},
    time::{Duration, Instant},
};

struct ChildGuard(Child);
impl Drop for ChildGuard {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

fn nonblocking(file: &impl AsRawFd) -> Result<(), String> {
    // These are live pipe descriptors owned by ChildStdout/ChildStderr.
    let flags = unsafe { libc::fcntl(file.as_raw_fd(), libc::F_GETFL) };
    if flags < 0
        || unsafe { libc::fcntl(file.as_raw_fd(), libc::F_SETFL, flags | libc::O_NONBLOCK) } < 0
    {
        return Err(std::io::Error::last_os_error().to_string());
    }
    Ok(())
}
fn drain(pipe: &mut impl Read, bytes: &mut Vec<u8>, limit: usize) -> Result<(), String> {
    let mut buffer = [0; 8192];
    loop {
        match pipe.read(&mut buffer) {
            Ok(0) => return Ok(()),
            Ok(count) => {
                if bytes.len() + count > limit {
                    return Err("Git output exceeded its bounded size limit".into());
                }
                bytes.extend_from_slice(&buffer[..count]);
            }
            Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => return Ok(()),
            Err(error) if error.kind() == std::io::ErrorKind::Interrupted => continue,
            Err(error) => return Err(error.to_string()),
        }
    }
}
fn git(cwd: &Path, arguments: &[&str], limit: usize) -> Result<Vec<u8>, String> {
    let mut command = Command::new("git");
    command
        .args([
            "--no-pager",
            "--no-optional-locks",
            "--literal-pathspecs",
            "-C",
        ])
        .arg(cwd)
        .args(arguments)
        .env("GIT_NO_REPLACE_OBJECTS", "1")
        .env("GIT_NO_LAZY_FETCH", "1")
        .env("GIT_ALLOW_PROTOCOL", "")
        .env("GIT_TERMINAL_PROMPT", "0")
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    for name in [
        "GIT_DIR",
        "GIT_WORK_TREE",
        "GIT_INDEX_FILE",
        "GIT_OBJECT_DIRECTORY",
        "GIT_ALTERNATE_OBJECT_DIRECTORIES",
        "GIT_COMMON_DIR",
    ] {
        command.env_remove(name);
    }
    let mut child = ChildGuard(
        command
            .spawn()
            .map_err(|error| format!("could not run Git: {error}"))?,
    );
    let mut stdout = child.0.stdout.take().expect("piped stdout");
    let mut stderr = child.0.stderr.take().expect("piped stderr");
    nonblocking(&stdout)?;
    nonblocking(&stderr)?;
    let deadline = Instant::now() + Duration::from_secs(10);
    let (mut output, mut errors) = (Vec::new(), Vec::new());
    loop {
        drain(&mut stdout, &mut output, limit)?;
        drain(&mut stderr, &mut errors, 16 * 1024)?;
        if let Some(status) = child.0.try_wait().map_err(|error| error.to_string())? {
            drain(&mut stdout, &mut output, limit)?;
            drain(&mut stderr, &mut errors, 16 * 1024)?;
            return if status.success() {
                Ok(output)
            } else {
                Err(format!(
                    "Git {status}: {}",
                    String::from_utf8_lossy(&errors)
                ))
            };
        }
        if Instant::now() >= deadline {
            return Err("Git command exceeded the 10-second deadline".into());
        }
        std::thread::sleep(Duration::from_millis(5));
    }
}

pub struct GitSource {
    pub root: PathBuf,
    pub relative: String,
}
impl GitSource {
    pub fn discover(source: &Path) -> Result<Self, String> {
        if std::fs::symlink_metadata(source).is_ok_and(|metadata| metadata.file_type().is_symlink())
        {
            return Err("Git comparisons of symlink entries are not supported".into());
        }
        let absolute = crate::source::canonical_location(source)?;
        let mut parent = absolute.parent().ok_or("source has no parent")?;
        while !parent.exists() {
            parent = parent.parent().ok_or("no existing source ancestor")?;
        }
        let output = git(parent, &["rev-parse", "--show-toplevel"], 16 * 1024)?;
        let output = std::str::from_utf8(&output).map_err(|_| "repository root is not UTF-8")?;
        let root = PathBuf::from(output.strip_suffix('\n').unwrap_or(output))
            .canonicalize()
            .map_err(|error| error.to_string())?;
        let relative = absolute
            .strip_prefix(&root)
            .map_err(|_| "source must be inside the repository")?
            .to_str()
            .ok_or("Git source path is not UTF-8")?
            .to_owned();
        Ok(Self { root, relative })
    }
    pub fn resolve(&self, revision: &str) -> Result<String, String> {
        if revision.is_empty() || revision.len() > 1024 || revision.contains('\0') {
            return Err("invalid Git revision".into());
        }
        let revision = format!("{revision}^{{commit}}");
        let output = git(
            &self.root,
            &["rev-parse", "--verify", "--end-of-options", &revision],
            256,
        )?;
        let commit = std::str::from_utf8(&output)
            .map_err(|_| "invalid commit ID")?
            .trim();
        if !matches!(commit.len(), 40 | 64) || !commit.bytes().all(|byte| byte.is_ascii_hexdigit())
        {
            return Err("Git did not resolve a commit ID".into());
        }
        Ok(commit.to_owned())
    }
    pub fn merge_base(&self, base: &str, head: &str) -> Result<String, String> {
        let output = git(&self.root, &["merge-base", "--all", base, head], 1024)?;
        let output = std::str::from_utf8(&output).map_err(|_| "invalid merge base")?;
        let bases: Vec<_> = output.lines().collect();
        if bases.len() != 1 {
            return Err(
                "comparison requires exactly one merge base; specify an exact --base instead"
                    .into(),
            );
        }
        self.resolve(bases[0])
    }
    pub fn read(&self, commit: &str) -> Result<Option<SourceText>, String> {
        // Resolve again to reject accidental unvalidated options/revisions at this public boundary.
        let commit = self.resolve(commit)?;
        let entry = git(
            &self.root,
            &[
                "ls-tree",
                "-z",
                "--full-tree",
                &commit,
                "--",
                &self.relative,
            ],
            32 * 1024,
        )?;
        if entry.is_empty() {
            return Ok(None);
        }
        let header = entry
            .split(|byte| *byte == b'\t')
            .next()
            .ok_or("invalid Git tree entry")?;
        let header = std::str::from_utf8(header).map_err(|_| "invalid Git tree entry")?;
        let fields: Vec<_> = header.split_whitespace().collect();
        if fields.len() != 3 || fields[1] != "blob" || fields[0] == "120000" {
            return Err(
                "comparison requires a regular Git blob, not a tree, symlink or submodule".into(),
            );
        }
        let object = fields[2];
        let size = git(&self.root, &["cat-file", "-s", object], 128)?;
        let size = std::str::from_utf8(&size)
            .ok()
            .and_then(|value| value.trim().parse::<usize>().ok())
            .ok_or("invalid Git blob size")?;
        if size > MAX_SOURCE_BYTES {
            return Err("Git blob exceeds the 1 MiB source limit".into());
        }
        let content = git(&self.root, &["cat-file", "blob", object], MAX_SOURCE_BYTES)?;
        if content.len() != size {
            return Err("Git blob size mismatch".into());
        }
        SourceText::parse(String::from_utf8(content).map_err(|_| "Git blob is not UTF-8")?)
            .map(Some)
    }
}
