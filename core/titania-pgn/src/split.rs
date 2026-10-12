//! Split: one file into several, by size or by what is in the tags.

use crate::error::{Error, Result, io_error, message};
use crate::header::{Header, decode, text, year};
use crate::output::{Claim, create_unique};
use crate::progress::{Meter, Sink, games};
use crate::scan::{Games, separator, walk_moves};
use crate::source::{Source, display};
use rustc_hash::{FxHashMap, FxHashSet};
use serde::{Deserialize, Serialize};
use std::fs::{self, File, OpenOptions};
use std::io::{BufWriter, Write};
use std::path::{Path, PathBuf};
use std::time::Instant;

/// Files kept open at once; the rest are reopened to append when their turn comes again.
const MAX_OPEN: usize = 100;
/// A split that would make more files than this is almost certainly a mistake.
const MAX_FILES: usize = 5000;
/// How many of the files are listed in the result.
const MAX_LISTED: usize = 500;

#[derive(Deserialize, Clone, Copy, PartialEq, Eq, Debug)]
#[serde(rename_all = "snake_case")]
pub enum SplitMode {
    /// `count` games in each file.
    Games,
    /// `count` files of equal size.
    Parts,
    Year,
    Event,
    Eco,
    Result,
    White,
    Black,
}

pub struct SplitOptions {
    pub input: PathBuf,
    pub output_dir: PathBuf,
    pub mode: SplitMode,
    pub count: u64,
}

#[derive(Serialize)]
pub struct SplitFile {
    pub name: String,
    pub games: u64,
}

#[derive(Serialize)]
pub struct SplitResult {
    pub output_dir: String,
    pub games: u64,
    pub files: u64,
    /// The files written, by name; at most the first 500.
    pub list: Vec<SplitFile>,
    pub elapsed_ms: u64,
}

/// How the games are shared out when splitting by size: the first `larger` files get `size + 1`
/// games each and the files after them `size`.
struct Sizes {
    size: u64,
    larger: u64,
    /// Digits in the file numbers.
    width: usize,
}

impl Sizes {
    /// The number, from zero, of the file that game `index` goes to.
    fn file_of(&self, index: u64) -> u64 {
        let boundary = self.larger * (self.size + 1);
        if index < boundary { index / (self.size + 1) } else { self.larger + (index - boundary) / self.size }
    }
}

pub fn split(options: &SplitOptions, sink: &mut dyn Sink) -> Result<SplitResult> {
    let started = Instant::now();
    let by_size = matches!(options.mode, SplitMode::Games | SplitMode::Parts);
    if by_size && options.count == 0 {
        return Err(message("The number of games or files must be at least 1."));
    }
    let source = Source::open(&options.input)?;
    let data = source.bytes();
    let stem = options.input.file_stem().map_or_else(|| "games".to_string(), |s| s.to_string_lossy().into_owned());
    fs::create_dir_all(&options.output_dir).map_err(|e| io_error("create the folder", &options.output_dir, e))?;
    let _claim = Claim::take(&options.output_dir.join(format!("{stem}_")))?;

    let mut meter = Meter::new(sink);
    // Splitting by size needs the number of games first, to size the parts and number the files.
    let mut sizes = Sizes { size: 1, larger: 0, width: 3 };
    if by_size {
        meter.start("Counting games", data.len() as u64)?;
        let mut total = 0u64;
        let mut scanner = Games::new(data);
        while scanner.next().is_some() {
            total += 1;
            meter.tick(scanner.position() as u64)?;
        }
        let files = match options.mode {
            SplitMode::Parts => {
                // Each file gets the same number of games, and the first few one more to use up
                // the remainder. With fewer games than files, every game gets a file of its own.
                let files = options.count.min(total).max(1);
                if files < options.count {
                    meter.log(&format!("There are only {}, so {} instead of {}.", games(total), files_text(files), options.count))?;
                }
                sizes.size = (total / files).max(1);
                sizes.larger = total % files;
                files
            }
            _ => {
                sizes.size = options.count;
                total.div_ceil(options.count)
            }
        };
        if files > MAX_FILES as u64 {
            return Err(too_many_files());
        }
        sizes.width = files.to_string().len().max(3);
    }

    meter.start("Splitting", data.len() as u64)?;
    // Everything is written to a folder of its own first, which the pool removes again if the
    // split fails or is cancelled. Files that are already there are only replaced at the end.
    let mut pool = Pool::create(&options.output_dir, &stem)?;
    let games = write_games(options.mode, data, &source.name(), &sizes, &mut pool, &mut meter)?;
    // Nothing may report to the sink from here on: once files are in place it is too late to cancel.
    let mut list: Vec<_> = pool.commit()?.into_iter().map(|(name, games)| SplitFile { name, games }).collect();

    list.sort_unstable_by(|a, b| a.name.cmp(&b.name));
    let files = list.len() as u64;
    list.truncate(MAX_LISTED);
    Ok(SplitResult { output_dir: display(&options.output_dir), games, files, list, elapsed_ms: started.elapsed().as_millis() as u64 })
}

fn write_games(mode: SplitMode, data: &[u8], file: &str, sizes: &Sizes, pool: &mut Pool, meter: &mut Meter) -> Result<u64> {
    let mut index = 0u64;
    let mut scanner = Games::new(data);
    while let Some(game) = scanner.next() {
        // What the games of one file have in common, and that as a piece of a file name.
        let (group, label) = match mode {
            SplitMode::Games | SplitMode::Parts => same(format!("{:0width$}", sizes.file_of(index) + 1, width = sizes.width)),
            _ => {
                let header = Header::read(game.tags);
                match mode {
                    SplitMode::Year => same(year(header.date()).map_or_else(|| "unknown-year".to_string(), |y| y.to_string())),
                    SplitMode::Event => by_value(text(&header.event), "unknown-event"),
                    SplitMode::Eco => by_value(text(&header.eco), "no-eco"),
                    SplitMode::White => by_value(text(&header.white), "unknown-player"),
                    SplitMode::Black => by_value(text(&header.black), "unknown-player"),
                    _ => {
                        // A game without a Result tag still has the result its moves end with.
                        let result = match text(&header.result) {
                            b"" => walk_moves(game.movetext, |_| {}).result.unwrap_or(b""),
                            result => result,
                        };
                        same(match result {
                            b"1-0" => "white-wins",
                            b"0-1" => "black-wins",
                            b"1/2-1/2" => "draws",
                            _ => "unfinished",
                        }
                        .to_string())
                    }
                }
            }
        };
        pool.write(group, &label, game.raw)?;
        index += 1;
        meter.tick(scanner.position() as u64)?;
    }
    meter.report(data.len() as u64)?;
    meter.log_unclosed(file, scanner.unclosed_comments())?;
    Ok(index)
}

fn same(label: String) -> (String, String) {
    (label.clone(), label)
}

/// Games belong together when the tag has the same value, give or take letter case and spaces
/// around it. The file name made from the value does not decide it: "A/B" and "A_B" both
/// become "A_B" there, and long values lose their ends.
fn by_value(value: &[u8], unknown: &str) -> (String, String) {
    let label = label(value, unknown);
    let group = decode(value).trim().to_lowercase();
    (if group.is_empty() { label.to_lowercase() } else { group }, label)
}

/// A tag value made safe to use in a file name.
fn label(value: &[u8], unknown: &str) -> String {
    let cleaned: String = decode(value)
        .chars()
        .map(|c| if c.is_control() || matches!(c, '<' | '>' | ':' | '"' | '/' | '\\' | '|' | '?' | '*') { '_' } else { c })
        .take(80)
        .collect();
    let cleaned = cleaned.trim_matches(|c: char| c.is_whitespace() || c == '.');
    if cleaned.is_empty() { unknown.to_string() } else { cleaned.to_string() }
}

fn files_text(count: u64) -> String {
    if count == 1 { "1 file is made".to_string() } else { format!("{count} files are made") }
}

fn too_many_files() -> Error {
    message(format!("This would create more than {MAX_FILES} files. Choose a different way to split."))
}

/// The output files of a split, of which only some are open at any moment. They are written
/// in a staging folder inside the output folder and moved into place together at the end.
struct Pool {
    dir: PathBuf,
    stem: String,
    staging: PathBuf,
    /// Set when files that were there before could not be put back; they are in the staging folder.
    keep_staging: bool,
    /// The open files, by their place in `files`.
    open: FxHashMap<usize, BufWriter<File>>,
    /// Where each group's file is in `files`.
    groups: FxHashMap<String, usize>,
    /// The file names in use, lower-cased: Windows file names ignore case.
    names: FxHashSet<String>,
    /// Every file written so far: its name and number of games.
    files: Vec<(String, u64)>,
}

impl Pool {
    fn create(dir: &Path, stem: &str) -> Result<Self> {
        let staging = create_unique(|unique| {
            let staging = dir.join(format!(".{stem}-split-{unique}.partial"));
            fs::create_dir(&staging).map(|()| staging)
        })
        .map_err(|e| io_error("write to the folder", dir, e))?;
        Ok(Self {
            dir: dir.to_path_buf(),
            stem: stem.to_string(),
            staging,
            keep_staging: false,
            open: FxHashMap::default(),
            groups: FxHashMap::default(),
            names: FxHashSet::default(),
            files: Vec::new(),
        })
    }

    fn write(&mut self, group: String, label: &str, game: &[u8]) -> Result<()> {
        let index = match self.groups.get(&group) {
            Some(&index) => index,
            None => {
                if self.files.len() >= MAX_FILES {
                    return Err(too_many_files());
                }
                // Two groups whose labels come out the same get different names: the later one a number.
                let mut name = format!("{}_{label}.pgn", self.stem);
                let mut number = 2;
                while !self.names.insert(name.to_lowercase()) {
                    name = format!("{}_{label} ({number}).pgn", self.stem);
                    number += 1;
                }
                self.files.push((name, 0));
                self.groups.insert(group, self.files.len() - 1);
                self.files.len() - 1
            }
        };
        let path = self.staging.join(&self.files[index].0);
        if !self.open.contains_key(&index) {
            if self.open.len() >= MAX_OPEN {
                self.close()?;
            }
            let file = OpenOptions::new().append(true).create(true).open(&path).map_err(|e| io_error("write", &path, e))?;
            self.open.insert(index, BufWriter::with_capacity(1 << 16, file));
        }
        let writer = self.open.get_mut(&index).expect("just opened");
        writer.write_all(game).and_then(|()| writer.write_all(separator(game))).map_err(|e| io_error("write", &path, e))?;
        self.files[index].1 += 1;
        Ok(())
    }

    fn close(&mut self) -> Result<()> {
        for (index, mut writer) in self.open.drain() {
            writer.flush().map_err(|e| io_error("write", &self.staging.join(&self.files[index].0), e))?;
        }
        Ok(())
    }

    /// Moves the finished files into the output folder and returns their names and numbers of
    /// games. Files of the same names that were there before are set aside first, and put back
    /// if anything goes wrong, so the folder ends up either all new or exactly as it was.
    fn commit(mut self) -> Result<Vec<(String, u64)>> {
        self.close()?;
        let replaced = self.staging.join("replaced");
        let mut set_aside: Vec<&str> = Vec::new();
        let mut placed: Vec<&str> = Vec::new();
        let mut failure = None;
        for (name, _) in &self.files {
            let target = self.dir.join(name);
            let moved = (|| -> std::io::Result<()> {
                if target.symlink_metadata().is_ok() {
                    fs::create_dir_all(&replaced)?;
                    fs::rename(&target, replaced.join(name))?;
                    set_aside.push(name);
                }
                fs::rename(self.staging.join(name), &target)?;
                placed.push(name);
                Ok(())
            })();
            if let Err(error) = moved {
                failure = Some(io_error("save", &target, error));
                break;
            }
        }
        let Some(error) = failure else { return Ok(std::mem::take(&mut self.files)) };

        // Put the folder back the way it was.
        for name in placed {
            let _ = fs::remove_file(self.dir.join(name));
        }
        let mut stranded = false;
        for name in set_aside {
            stranded |= fs::rename(replaced.join(name), self.dir.join(name)).is_err();
        }
        if !stranded {
            return Err(error);
        }
        self.keep_staging = true;
        Err(message(format!("{error} Some of the files that were replaced could not be put back; they are in {}.", replaced.display())))
    }
}

impl Drop for Pool {
    /// Removes the staging folder with whatever is left in it: everything written, after a
    /// failure or a cancel, and the files that were replaced, after a split that went through.
    fn drop(&mut self) {
        self.open.clear();
        if !self.keep_staging {
            let _ = fs::remove_dir_all(&self.staging);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn labels_are_safe_file_names() {
        assert_eq!(label(b"Linares 1994", "x"), "Linares 1994");
        assert_eq!(label(b"A/B: \"C\"?", "x"), "A_B_ _C__");
        assert_eq!(label(b" ... ", "x"), "x");
        assert_eq!(label(b"", "x"), "x");
        assert_eq!(label(b"Wch U20 ", "x"), "Wch U20");
    }

    #[test]
    fn groups_follow_the_value_and_not_the_file_name() {
        assert_ne!(by_value(b"A/B", "x").0, by_value(b"A_B", "x").0);
        assert_eq!(by_value(b"A/B", "x").1, by_value(b"A_B", "x").1);
        assert_eq!(by_value(b"Linares ", "x").0, by_value(b"LINARES", "x").0);
        assert_eq!(by_value(b"", "unknown-event"), ("unknown-event".to_string(), "unknown-event".to_string()));
    }

    #[test]
    fn parts_share_the_games_out_evenly() {
        // Ten games in six files: 2, 2, 2, 2, 1, 1.
        let sizes = Sizes { size: 10 / 6, larger: 10 % 6, width: 3 };
        let files: Vec<u64> = (0..10).map(|game| sizes.file_of(game)).collect();
        assert_eq!(files, [0, 0, 1, 1, 2, 2, 3, 3, 4, 5]);
    }
}
