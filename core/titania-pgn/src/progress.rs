use crate::error::{Error, Result};

/// Receives progress from a running operation. Every method returns `false` to cancel it.
pub trait Sink {
    /// `done` of `total` units are finished and `games` games have been handled in this phase.
    /// A `total` of zero means the amount of work is unknown.
    fn progress(&mut self, done: u64, total: u64, games: u64) -> bool;
    /// A new phase has started, e.g. "Reading a.pgn".
    fn phase(&mut self, text: &str) -> bool;
    /// A line for the log.
    fn log(&mut self, text: &str) -> bool;
}

/// A sink that ignores everything and never cancels.
pub struct NoSink;

impl Sink for NoSink {
    fn progress(&mut self, _done: u64, _total: u64, _games: u64) -> bool {
        true
    }
    fn phase(&mut self, _text: &str) -> bool {
        true
    }
    fn log(&mut self, _text: &str) -> bool {
        true
    }
}

/// How many games pass between progress reports.
const GAMES_PER_REPORT: u32 = 512;

/// Counts games and reports to the sink every so often, turning a refusal into `Error::Cancelled`.
pub(crate) struct Meter<'a> {
    sink: &'a mut dyn Sink,
    total: u64,
    games: u64,
    pending: u32,
}

impl<'a> Meter<'a> {
    pub fn new(sink: &'a mut dyn Sink) -> Self {
        Self { sink, total: 0, games: 0, pending: 0 }
    }

    /// Begins a phase of `total` units. Zero means the amount of work is unknown.
    pub fn start(&mut self, phase: &str, total: u64) -> Result<()> {
        self.total = total;
        self.games = 0;
        self.pending = 0;
        proceed(self.sink.phase(phase))?;
        self.report(0)
    }

    /// One more game is done, and `done` units with it.
    pub fn tick(&mut self, done: u64) -> Result<()> {
        self.games += 1;
        self.pending += 1;
        if self.pending < GAMES_PER_REPORT {
            return Ok(());
        }
        self.pending = 0;
        self.report(done)
    }

    pub fn report(&mut self, done: u64) -> Result<()> {
        proceed(self.sink.progress(done, self.total, self.games))
    }

    pub fn log(&mut self, text: &str) -> Result<()> {
        proceed(self.sink.log(text))
    }

    /// Notes in the log that the scanner had to cut games off inside comments that never closed.
    pub fn log_unclosed(&mut self, file: &str, comments: u64) -> Result<()> {
        match comments {
            0 => Ok(()),
            1 => self.log(&format!("{file}: 1 comment was never closed; it was ended where the next game starts.")),
            n => self.log(&format!("{file}: {} comments were never closed; they were ended where the next game starts.", grouped(n))),
        }
    }
}

fn proceed(go_on: bool) -> Result<()> {
    if go_on { Ok(()) } else { Err(Error::Cancelled) }
}

/// 1234567 -> "1,234,567"
pub(crate) fn grouped(value: u64) -> String {
    let digits = value.to_string();
    let mut out = String::with_capacity(digits.len() + digits.len() / 3);
    for (i, c) in digits.chars().enumerate() {
        if i > 0 && (digits.len() - i) % 3 == 0 {
            out.push(',');
        }
        out.push(c);
    }
    out
}

pub(crate) fn games(count: u64) -> String {
    if count == 1 { "1 game".to_string() } else { format!("{} games", grouped(count)) }
}
