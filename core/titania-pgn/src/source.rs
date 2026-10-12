use crate::error::{Result, io_error, message};
use memmap2::Mmap;
use std::fs::{File, OpenOptions};
use std::path::{Path, PathBuf};

/// A PGN file mapped into memory for reading. While it is open, no other program can write
/// to the file, rename it or delete it.
pub(crate) struct Source {
    pub path: PathBuf,
    map: Option<Mmap>,
    /// Holds the file against changes for as long as it is being read.
    _file: File,
}

/// Opens a file for reading so that others may read it too, but not change it. Games are
/// found once and copied later by their place in the file, so the file has to stay as it is.
fn open_shared(path: &Path) -> std::io::Result<File> {
    let mut options = OpenOptions::new();
    options.read(true);
    #[cfg(windows)]
    {
        use std::os::windows::fs::OpenOptionsExt;
        const FILE_SHARE_READ: u32 = 1;
        options.share_mode(FILE_SHARE_READ);
    }
    let file = options.open(path)?;
    // Elsewhere a lock is only advisory, but it is the most that can be asked for.
    #[cfg(not(windows))]
    file.try_lock_shared().map_err(std::io::Error::from)?;
    Ok(file)
}

impl Source {
    pub fn open(path: &Path) -> Result<Self> {
        let file = open_shared(path).map_err(|e| io_error("open", path, e))?;
        let len = file.metadata().map_err(|e| io_error("read", path, e))?.len();
        // An empty file cannot be mapped.
        let map = if len == 0 {
            None
        } else {
            // SAFETY: the map is read-only, and `open_shared` keeps other programs from
            // changing or truncating the file for as long as the handle is held.
            Some(unsafe { Mmap::map(&file) }.map_err(|e| io_error("read", path, e))?)
        };
        let source = Self { path: path.to_path_buf(), map, _file: file };
        let raw = source.raw();
        if raw.starts_with(&[0xFF, 0xFE]) || raw.starts_with(&[0xFE, 0xFF]) {
            return Err(message(format!(
                "{} is saved as UTF-16, which PGN tools cannot read. Save it again as UTF-8 first.",
                source.name()
            )));
        }
        Ok(source)
    }

    fn raw(&self) -> &[u8] {
        self.map.as_deref().unwrap_or(&[])
    }

    /// The text of the file without a UTF-8 byte order mark.
    pub fn bytes(&self) -> &[u8] {
        let raw = self.raw();
        raw.strip_prefix(b"\xEF\xBB\xBF").unwrap_or(raw)
    }

    pub fn has_bom(&self) -> bool {
        self.raw().starts_with(b"\xEF\xBB\xBF")
    }

    pub fn file_size(&self) -> u64 {
        self.raw().len() as u64
    }

    pub fn name(&self) -> String {
        file_name(&self.path)
    }
}

pub(crate) fn open_all(paths: &[PathBuf]) -> Result<Vec<Source>> {
    if paths.is_empty() {
        return Err(message("No files were given."));
    }
    paths.iter().map(|p| Source::open(p)).collect()
}

pub(crate) fn file_name(path: &Path) -> String {
    path.file_name().map_or_else(|| path.display().to_string(), |n| n.to_string_lossy().into_owned())
}

pub(crate) fn display(path: &Path) -> String {
    path.display().to_string()
}
