use crate::error::{Result, io_error, message};
use crate::source::file_name;
use std::fs::{self, File, OpenOptions};
use std::io::{self, BufWriter, Write};
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

/// A file that is written under a temporary name and only takes its real name once it is
/// complete, so a failed or cancelled run never leaves half a file or destroys an existing one.
pub(crate) struct Output {
    path: PathBuf,
    partial: PathBuf,
    writer: Option<BufWriter<File>>,
    bytes: u64,
    committed: bool,
    _claim: Claim,
}

impl Output {
    pub fn create(path: &Path) -> Result<Self> {
        let name = path.file_name().ok_or_else(|| message("The output needs a file name."))?;
        let claim = Claim::take(path)?;
        // Each run writes to a temporary file of its own.
        let (partial, file) = create_unique(|unique| {
            let mut name = name.to_os_string();
            name.push(format!(".{unique}.partial"));
            let partial = path.with_file_name(name);
            OpenOptions::new().write(true).create_new(true).open(&partial).map(|file| (partial, file))
        })
        .map_err(|e| io_error("create", path, e))?;
        Ok(Self { path: path.to_path_buf(), partial, writer: Some(BufWriter::with_capacity(1 << 20, file)), bytes: 0, committed: false, _claim: claim })
    }

    pub fn write(&mut self, bytes: &[u8]) -> Result<()> {
        let writer = self.writer.as_mut().expect("output is open");
        writer.write_all(bytes).map_err(|e| io_error("write", &self.path, e))?;
        self.bytes += bytes.len() as u64;
        Ok(())
    }

    /// Finishes the file and gives it its real name. Returns the number of bytes written.
    pub fn commit(mut self) -> Result<u64> {
        let mut writer = self.writer.take().expect("output is open");
        writer.flush().map_err(|e| io_error("write", &self.path, e))?;
        drop(writer);
        fs::rename(&self.partial, &self.path).map_err(|e| io_error("save", &self.path, e))?;
        self.committed = true;
        Ok(self.bytes)
    }
}

impl Drop for Output {
    fn drop(&mut self) {
        // Not committed means the run failed or was cancelled, or the file could not be saved.
        if !self.committed {
            drop(self.writer.take());
            let _ = fs::remove_file(&self.partial);
        }
    }
}

/// Calls `create` with one unique name after another until it makes something new.
pub(crate) fn create_unique<T>(mut create: impl FnMut(&str) -> io::Result<T>) -> io::Result<T> {
    static COUNTER: AtomicU64 = AtomicU64::new(0);
    loop {
        let nanos = SystemTime::now().duration_since(UNIX_EPOCH).map_or(0, |d| d.subsec_nanos());
        let unique = format!("{}-{}-{nanos:x}", std::process::id(), COUNTER.fetch_add(1, Ordering::Relaxed));
        match create(&unique) {
            Err(e) if e.kind() == io::ErrorKind::AlreadyExists => {}
            other => return other,
        }
    }
}

/// The destinations being written to by operations running in this process.
static CLAIMED: Mutex<Vec<String>> = Mutex::new(Vec::new());

/// Holds a destination for one operation, so that two of them cannot replace the same files
/// at the same time. Released when dropped.
pub(crate) struct Claim(String);

impl Claim {
    pub fn take(path: &Path) -> Result<Self> {
        let full = resolve(path).to_string_lossy().into_owned();
        let key = if cfg!(windows) { full.to_lowercase() } else { full };
        let mut claimed = CLAIMED.lock().unwrap_or_else(|e| e.into_inner());
        if claimed.contains(&key) {
            return Err(message(format!("{} is being written by another operation. Wait for that to finish first.", file_name(path))));
        }
        claimed.push(key.clone());
        Ok(Self(key))
    }
}

impl Drop for Claim {
    fn drop(&mut self) {
        CLAIMED.lock().unwrap_or_else(|e| e.into_inner()).retain(|key| *key != self.0);
    }
}

/// Refuses an output file that is also one of the inputs: it would be overwritten while being read.
pub(crate) fn check_output(output: &Path, inputs: &[PathBuf]) -> Result<()> {
    let target = resolve(output);
    for input in inputs {
        if same_path(&resolve(input), &target) {
            return Err(message(format!(
                "{} is one of the files being read. Choose a different name for the new file.",
                file_name(output)
            )));
        }
    }
    Ok(())
}

/// The full path of a file that may not exist yet.
fn resolve(path: &Path) -> PathBuf {
    if let Ok(full) = fs::canonicalize(path) {
        return full;
    }
    let parent = path.parent().filter(|p| !p.as_os_str().is_empty()).unwrap_or(Path::new("."));
    match (fs::canonicalize(parent), path.file_name()) {
        (Ok(parent), Some(name)) => parent.join(name),
        _ => path.to_path_buf(),
    }
}

fn same_path(a: &Path, b: &Path) -> bool {
    if cfg!(windows) { a.to_string_lossy().eq_ignore_ascii_case(&b.to_string_lossy()) } else { a == b }
}
