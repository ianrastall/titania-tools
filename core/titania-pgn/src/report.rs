//! A summary of what is in a PGN file.

use crate::error::Result;
use crate::header::{best_date, decode, elo, is_unknown, year};
use crate::progress::{Meter, Sink, games};
use crate::scan::{Games, Tags, walk_moves};
use crate::source::{Source, display};
use rustc_hash::FxHashMap;
use serde::Serialize;
use std::path::Path;
use std::time::Instant;

/// How many entries the "top" lists hold.
const TOP: usize = 15;
/// How many distinct tag names are listed.
const TOP_TAGS: usize = 40;

#[derive(Serialize)]
pub struct Count {
    pub name: String,
    pub games: u64,
}

#[derive(Serialize, Default)]
pub struct Report {
    pub path: String,
    pub file_size: u64,
    pub games: u64,
    pub elapsed_ms: u64,
    pub encoding: String,
    pub line_endings: String,

    pub white_wins: u64,
    pub black_wins: u64,
    pub draws: u64,
    pub other_results: u64,

    pub first_date: Option<String>,
    pub last_date: Option<String>,
    /// Games per year, oldest first.
    pub years: Vec<Count>,
    pub undated: u64,

    pub players: u64,
    pub top_players: Vec<Count>,
    pub events: u64,
    pub top_events: Vec<Count>,
    pub eco_codes: u64,
    pub top_eco: Vec<Count>,

    /// Games in which at least one player has a rating.
    pub rated_games: u64,
    pub min_elo: u32,
    pub max_elo: u32,
    pub average_elo: f64,

    pub total_plies: u64,
    pub min_plies: u32,
    pub max_plies: u32,
    pub without_moves: u64,
    pub with_comments: u64,
    pub with_variations: u64,
    /// Games missing one of the seven required tags.
    pub incomplete_headers: u64,
    /// Games that start from a set-up position.
    pub from_position: u64,

    /// How many games carry each tag, most common first.
    pub tags: Vec<Count>,
}

type Counts = FxHashMap<Box<[u8]>, u64>;

fn bump(counts: &mut Counts, key: &[u8]) {
    match counts.get_mut(key) {
        Some(n) => *n += 1,
        None => {
            counts.insert(key.into(), 1);
        }
    }
}

fn top(counts: Counts, limit: usize) -> Vec<Count> {
    let mut list: Vec<_> = counts.into_iter().collect();
    list.sort_unstable_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.cmp(&b.0)));
    list.truncate(limit);
    list.into_iter().map(|(name, games)| Count { name: decode(&name), games }).collect()
}

pub fn report(input: &Path, sink: &mut dyn Sink) -> Result<Report> {
    let started = Instant::now();
    let source = Source::open(input)?;
    let data = source.bytes();
    let mut meter = Meter::new(sink);
    meter.start(&format!("Reading {}", source.name()), data.len() as u64)?;

    let mut r = Report { path: display(input), file_size: source.file_size(), min_plies: u32::MAX, min_elo: u32::MAX, ..Report::default() };
    let (mut players, mut events, mut ecos, mut tag_names) = (Counts::default(), Counts::default(), Counts::default(), Counts::default());
    let mut years: FxHashMap<u16, u64> = FxHashMap::default();
    let (mut first_date, mut last_date): (Option<Vec<u8>>, Option<Vec<u8>>) = (None, None);
    let (mut ascii, mut utf8) = (true, true);
    let (mut elo_sum, mut elo_count) = (0u64, 0u64);

    let mut scanner = Games::new(data);
    while let Some(game) = scanner.next() {
        meter.tick(scanner.position() as u64)?;
        r.games += 1;

        if ascii && !game.raw.is_ascii() {
            ascii = false;
        }
        if !ascii && utf8 && std::str::from_utf8(game.raw).is_err() {
            utf8 = false;
        }

        let mut roster = 0u8;
        let mut rated = false;
        let (mut date, mut utc_date) = (None, None);
        let mut result = None;
        for (name, value) in Tags::new(game.tags) {
            bump(&mut tag_names, name);
            let known = !is_unknown(&value);
            match name {
                b"Event" => {
                    roster |= 1;
                    if known {
                        bump(&mut events, &value);
                    }
                }
                b"Site" => roster |= 2,
                b"Date" => {
                    roster |= 4;
                    date = Some(value);
                }
                b"Round" => roster |= 8,
                b"White" | b"Black" => {
                    roster |= if name == b"White" { 16 } else { 32 };
                    if known {
                        bump(&mut players, &value);
                    }
                }
                b"Result" => {
                    roster |= 64;
                    result = Some(value);
                }
                b"UTCDate" => utc_date = Some(value),
                b"ECO" if known => bump(&mut ecos, &value),
                b"WhiteElo" | b"BlackElo" => {
                    let rating = elo(&value);
                    if rating > 0 {
                        rated = true;
                        elo_sum += u64::from(rating);
                        elo_count += 1;
                        r.min_elo = r.min_elo.min(rating);
                        r.max_elo = r.max_elo.max(rating);
                    }
                }
                b"FEN" if known => r.from_position += 1,
                _ => {}
            }
        }
        r.incomplete_headers += u64::from(roster != 127);
        r.rated_games += u64::from(rated);

        let date = best_date(date.as_deref().unwrap_or(b""), utc_date.as_deref().unwrap_or(b""));
        match year(date) {
            Some(y) => {
                *years.entry(y).or_default() += 1;
                let date = &date[..date.len().min(10)];
                if first_date.as_deref().is_none_or(|d| date < d) {
                    first_date = Some(date.to_vec());
                }
                if last_date.as_deref().is_none_or(|d| date > d) {
                    last_date = Some(date.to_vec());
                }
            }
            None => r.undated += 1,
        }

        let moves = walk_moves(game.movetext, |_| {});
        // A game without a Result tag still has the result its moves end with.
        match result.as_deref().filter(|value| !is_unknown(value)).or(moves.result) {
            Some(b"1-0") => r.white_wins += 1,
            Some(b"0-1") => r.black_wins += 1,
            Some(b"1/2-1/2") => r.draws += 1,
            _ => r.other_results += 1,
        }
        r.total_plies += u64::from(moves.plies);
        r.min_plies = r.min_plies.min(moves.plies);
        r.max_plies = r.max_plies.max(moves.plies);
        r.without_moves += u64::from(moves.plies == 0);
        r.with_comments += u64::from(moves.has_comments);
        r.with_variations += u64::from(moves.has_variations);
    }
    meter.report(data.len() as u64)?;

    if r.games == 0 {
        r.min_plies = 0;
    }
    if elo_count == 0 {
        r.min_elo = 0;
    } else {
        r.average_elo = elo_sum as f64 / elo_count as f64;
    }

    r.encoding = match (ascii, utf8, source.has_bom()) {
        (_, true, true) => "UTF-8 with byte order mark",
        (true, _, _) => "ASCII",
        (false, true, _) => "UTF-8",
        (false, false, _) => "Latin-1 (not UTF-8)",
    }
    .to_string();
    r.line_endings = line_endings(data).to_string();

    r.first_date = first_date.map(|d| decode(&d));
    r.last_date = last_date.map(|d| decode(&d));
    let mut by_year: Vec<_> = years.into_iter().collect();
    by_year.sort_unstable();
    r.years = by_year.into_iter().map(|(y, games)| Count { name: y.to_string(), games }).collect();

    r.players = players.len() as u64;
    r.events = events.len() as u64;
    r.eco_codes = ecos.len() as u64;
    r.top_players = top(players, TOP);
    r.top_events = top(events, TOP);
    r.top_eco = top(ecos, TOP);
    r.tags = top(tag_names, TOP_TAGS);

    r.elapsed_ms = started.elapsed().as_millis() as u64;
    meter.log(&format!("{}: {}", source.name(), games(r.games)))?;
    meter.log_unclosed(&source.name(), scanner.unclosed_comments())?;
    Ok(r)
}

/// Judged from the first megabyte, which is plenty to tell.
fn line_endings(data: &[u8]) -> &'static str {
    let sample = &data[..data.len().min(1 << 20)];
    let lf = memchr::memchr_iter(b'\n', sample).count();
    let crlf = memchr::memmem::find_iter(sample, b"\r\n").count();
    match (lf, crlf) {
        (0, _) => "None",
        (_, 0) => "Unix (LF)",
        (lf, crlf) if lf == crlf => "Windows (CRLF)",
        _ => "Mixed",
    }
}
