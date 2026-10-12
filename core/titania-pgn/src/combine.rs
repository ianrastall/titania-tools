//! Merge and sort. Both read every game of the input files into an index (where the game is and
//! what it sorts by), optionally drop duplicates and reorder, then copy the games out. Memory use
//! follows the number of games, not the size of the files.

use crate::error::{Result, message};
use crate::header::{Header, elo, number, text};
use crate::output::{Output, check_output};
use crate::progress::{Meter, Sink, games};
use crate::scan::{Games, separator, walk_moves};
use crate::source::{display, file_name, open_all};
use rustc_hash::FxHashSet;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use std::time::Instant;
use xxhash_rust::xxh3::Xxh3;

#[derive(Deserialize, Clone, Copy, PartialEq, Eq, Debug)]
#[serde(rename_all = "snake_case")]
pub enum Field {
    Date,
    Event,
    Site,
    Round,
    White,
    Black,
    Result,
    Eco,
    WhiteElo,
    BlackElo,
    AverageElo,
    /// Length of the game in half-moves.
    Moves,
}

#[derive(Deserialize, Clone, Copy, Debug)]
pub struct SortKey {
    pub field: Field,
    #[serde(default)]
    pub descending: bool,
}

pub struct CombineOptions {
    pub inputs: Vec<PathBuf>,
    pub output: PathBuf,
    /// Leave out games already seen: same players, same date, same starting position, same moves.
    pub remove_duplicates: bool,
    /// Sort order; empty keeps the games in the order they were read.
    pub keys: Vec<SortKey>,
}

#[derive(Serialize)]
pub struct CombineResult {
    pub output: String,
    pub files: u64,
    pub games_read: u64,
    pub games_written: u64,
    pub duplicates_removed: u64,
    pub bytes_written: u64,
    pub elapsed_ms: u64,
}

/// How many games are sorted between two looks at whether the run has been cancelled.
const SORT_RUN: usize = 1 << 16;

/// Where a game is, and where its sort key is in the key arena.
#[derive(Clone, Copy)]
struct Entry {
    offset: u64,
    key: u64,
    source: u32,
    len: u32,
    key_len: u32,
}

pub fn combine(options: &CombineOptions, sink: &mut dyn Sink) -> Result<CombineResult> {
    let started = Instant::now();
    check_output(&options.output, &options.inputs)?;
    let sources = open_all(&options.inputs)?;
    let total: u64 = sources.iter().map(|s| s.bytes().len() as u64).sum();

    let keys = &options.keys;
    let dedupe = options.remove_duplicates;
    let need_header = dedupe || !keys.is_empty();
    let need_plies = keys.iter().any(|k| k.field == Field::Moves);

    let mut entries: Vec<Entry> = Vec::new();
    let mut arena: Vec<u8> = Vec::new();
    let mut seen: FxHashSet<u128> = FxHashSet::default();
    let mut hasher = Xxh3::new();
    let mut games_read = 0u64;
    let mut duplicates = 0u64;
    let mut done = 0u64;
    let mut meter = Meter::new(sink);

    for (index, source) in sources.iter().enumerate() {
        let data = source.bytes();
        meter.start(&format!("Reading {}", source.name()), total)?;
        let mut count = 0u64;
        let mut dropped = 0u64;
        let mut scanner = Games::new(data);
        while let Some(game) = scanner.next() {
            meter.tick(done + scanner.position() as u64)?;
            count += 1;
            let header = if need_header { Header::read(game.tags) } else { Header::default() };
            let mut plies = 0;
            if dedupe {
                hasher.reset();
                hash_text(&mut hasher, text(&header.white));
                hash_text(&mut hasher, text(&header.black));
                hash_text(&mut hasher, header.date());
                // The same moves from another position, or under other rules, are another game.
                hash_text(&mut hasher, header.start_position());
                hash_text(&mut hasher, header.variant());
                plies = walk_moves(game.movetext, |mv| {
                    hasher.update(mv);
                    hasher.update(b" ");
                })
                .plies;
                // Games without moves only count as duplicates when they are identical throughout.
                if plies == 0 {
                    hasher.update(game.raw);
                }
                if !seen.insert(hasher.digest128()) {
                    dropped += 1;
                    continue;
                }
            } else if need_plies {
                plies = walk_moves(game.movetext, |_| {}).plies;
            }

            let key = arena.len();
            push_key(&mut arena, keys, &header, plies);
            let len = u32::try_from(game.raw.len())
                .map_err(|_| message(format!("{} has a game larger than 4 GB.", source.name())))?;
            entries.push(Entry { offset: game.offset as u64, key: key as u64, source: index as u32, len, key_len: (arena.len() - key) as u32 });
        }
        done += data.len() as u64;
        games_read += count;
        duplicates += dropped;
        meter.log(&match dropped {
            0 => format!("{}: {}", source.name(), games(count)),
            n => format!("{}: {}, {} already seen", source.name(), games(count), crate::progress::grouped(n)),
        })?;
        meter.log_unclosed(&source.name(), scanner.unclosed_comments())?;
    }
    drop(seen);

    if !keys.is_empty() {
        meter.start("Sorting", 0)?;
        sort_entries(&mut entries, &arena, &mut meter)?;
    }

    let to_write: u64 = entries.iter().map(|e| u64::from(e.len)).sum();
    meter.start(&format!("Writing {}", file_name(&options.output)), to_write)?;
    let mut output = Output::create(&options.output)?;
    let mut written = 0u64;
    for entry in &entries {
        let data = sources[entry.source as usize].bytes();
        let game = &data[entry.offset as usize..entry.offset as usize + entry.len as usize];
        output.write(game)?;
        output.write(separator(game))?;
        written += u64::from(entry.len);
        meter.tick(written)?;
    }
    meter.report(written)?;
    // Nothing may report to the sink from here on: once the file is saved it is too late to cancel.
    let bytes_written = output.commit()?;

    Ok(CombineResult {
        output: display(&options.output),
        files: sources.len() as u64,
        games_read,
        games_written: entries.len() as u64,
        duplicates_removed: duplicates,
        bytes_written,
        elapsed_ms: started.elapsed().as_millis() as u64,
    })
}

/// A stable merge sort: games that compare equal stay in the order they were read. It reports
/// to the meter between its steps, so that sorting a very large file can be cancelled.
fn sort_entries(entries: &mut Vec<Entry>, arena: &[u8], meter: &mut Meter) -> Result<()> {
    let key_of = |e: &Entry| &arena[e.key as usize..e.key as usize + e.key_len as usize];
    for run in entries.chunks_mut(SORT_RUN) {
        run.sort_by(|a, b| key_of(a).cmp(key_of(b)));
        meter.report(0)?;
    }
    let mut width = SORT_RUN;
    let mut merged: Vec<Entry> = Vec::new();
    while width < entries.len() {
        merged.clear();
        merged.reserve_exact(entries.len());
        for pair in entries.chunks(2 * width) {
            let (left, right) = pair.split_at(width.min(pair.len()));
            let (mut l, mut r) = (0, 0);
            while l < left.len() && r < right.len() {
                // Taking from the left when the two are equal is what keeps the sort stable.
                if key_of(&right[r]) < key_of(&left[l]) {
                    merged.push(right[r]);
                    r += 1;
                } else {
                    merged.push(left[l]);
                    l += 1;
                }
            }
            merged.extend_from_slice(&left[l..]);
            merged.extend_from_slice(&right[r..]);
            meter.report(0)?;
        }
        std::mem::swap(entries, &mut merged);
        width *= 2;
    }
    Ok(())
}

/// Hashes a tag value without regard to letter case, followed by a separator.
fn hash_text(hasher: &mut Xxh3, value: &[u8]) {
    let mut buffer = [0u8; 64];
    for chunk in value.chunks(buffer.len()) {
        let lowered = &mut buffer[..chunk.len()];
        lowered.copy_from_slice(chunk);
        lowered.make_ascii_lowercase();
        hasher.update(lowered);
    }
    hasher.update(&[0]);
}

/// Appends the game's sort key: one part per sort field, encoded so that comparing the bytes of
/// two keys compares the games. Descending parts are stored inverted.
fn push_key(arena: &mut Vec<u8>, keys: &[SortKey], header: &Header, plies: u32) {
    for key in keys {
        let at = arena.len();
        match key.field {
            Field::Date => arena.extend_from_slice(&date_key(header.date())),
            Field::Round => arena.extend_from_slice(&round_key(text(&header.round))),
            Field::Event => push_text(arena, text(&header.event)),
            Field::Site => push_text(arena, text(&header.site)),
            Field::White => push_text(arena, text(&header.white)),
            Field::Black => push_text(arena, text(&header.black)),
            Field::Result => push_text(arena, text(&header.result)),
            Field::Eco => push_text(arena, text(&header.eco)),
            Field::WhiteElo => arena.extend_from_slice(&elo(text(&header.white_elo)).to_be_bytes()),
            Field::BlackElo => arena.extend_from_slice(&elo(text(&header.black_elo)).to_be_bytes()),
            Field::AverageElo => {
                let (white, black) = (elo(text(&header.white_elo)), elo(text(&header.black_elo)));
                let average = if white > 0 && black > 0 { (white + black) / 2 } else { white.max(black) };
                arena.extend_from_slice(&average.to_be_bytes());
            }
            Field::Moves => arena.extend_from_slice(&plies.to_be_bytes()),
        }
        if key.descending {
            arena[at..].iter_mut().for_each(|b| *b = !*b);
        }
    }
}

/// Text sorts without regard to letter case. The zero terminator makes a shorter name sort
/// before a longer one that starts with it.
fn push_text(arena: &mut Vec<u8>, value: &[u8]) {
    arena.extend(value.iter().map(|b| b.to_ascii_lowercase().max(1)));
    arena.push(0);
}

/// "1998.07.??" -> year, month, day, with zero for unknown parts so they sort first.
fn date_key(date: &[u8]) -> [u8; 4] {
    let mut parts = date.split(|b| matches!(b, b'.' | b'-' | b'/')).map(number);
    let year = parts.next().unwrap_or(0).min(u32::from(u16::MAX)) as u16;
    let month = parts.next().unwrap_or(0).min(255) as u8;
    let day = parts.next().unwrap_or(0).min(255) as u8;
    let [high, low] = year.to_be_bytes();
    [high, low, month, day]
}

/// "3.12" -> round 3, board 12, so that round 10 sorts after round 9.
fn round_key(round: &[u8]) -> [u8; 6] {
    let mut key = [0u8; 6];
    for (slot, part) in key.chunks_exact_mut(2).zip(round.split(|b| *b == b'.')) {
        slot.copy_from_slice(&(number(part).min(u32::from(u16::MAX)) as u16).to_be_bytes());
    }
    key
}

#[cfg(test)]
mod tests {
    use super::*;

    fn key(fields: &[(Field, bool)], tags: &str) -> Vec<u8> {
        let keys: Vec<_> = fields.iter().map(|&(field, descending)| SortKey { field, descending }).collect();
        let mut arena = Vec::new();
        push_key(&mut arena, &keys, &Header::read(tags.as_bytes()), 0);
        arena
    }

    #[test]
    fn the_merge_sort_is_stable_and_can_be_cancelled() {
        // Keys repeat, so the offset tells whether equal games kept their order.
        let count = SORT_RUN * 5 + 123;
        let arena: Vec<u8> = (0..count).map(|i| (i * 7919 % 251) as u8).collect();
        let unsorted: Vec<Entry> = (0..count).map(|i| Entry { offset: i as u64, key: i as u64, source: 0, len: 0, key_len: 1 }).collect();

        let mut entries = unsorted.clone();
        sort_entries(&mut entries, &arena, &mut Meter::new(&mut crate::progress::NoSink)).unwrap();
        assert_eq!(entries.len(), count);
        assert!(entries.windows(2).all(|w| (arena[w[0].key as usize], w[0].offset) < (arena[w[1].key as usize], w[1].offset)));

        struct StopAfter(u32);
        impl Sink for StopAfter {
            fn progress(&mut self, _done: u64, _total: u64, _games: u64) -> bool {
                self.0 = self.0.saturating_sub(1);
                self.0 > 0
            }
            fn phase(&mut self, _text: &str) -> bool {
                true
            }
            fn log(&mut self, _text: &str) -> bool {
                true
            }
        }
        let mut entries = unsorted;
        let stopped = sort_entries(&mut entries, &arena, &mut Meter::new(&mut StopAfter(8)));
        assert!(matches!(stopped, Err(crate::error::Error::Cancelled)));
    }

    #[test]
    fn dates_sort_by_time_with_unknown_first() {
        let k = |d: &str| key(&[(Field::Date, false)], &format!("[Date \"{d}\"]"));
        assert!(k("????.??.??") < k("1851.??.??"));
        assert!(k("1851.??.??") < k("1851.05.27"));
        assert!(k("1851.05.27") < k("1851.11.02"));
        assert!(k("1999.12.31") < k("2000.01.01"));
    }

    #[test]
    fn rounds_sort_as_numbers() {
        let k = |r: &str| key(&[(Field::Round, false)], &format!("[Round \"{r}\"]"));
        assert!(k("9") < k("10"));
        assert!(k("3.2") < k("3.12"));
        assert!(k("?") < k("1"));
    }

    #[test]
    fn text_ignores_case_and_handles_prefixes() {
        let k = |w: &str, descending| key(&[(Field::White, descending)], &format!("[White \"{w}\"]"));
        assert_eq!(k("carlsen", false), k("Carlsen", false));
        assert!(k("Lasker", false) < k("Lasker, Ed", false));
        assert!(k("Lasker", true) > k("Lasker, Ed", true));
        assert!(k("Anand", true) > k("Zukertort", true));
    }
}
