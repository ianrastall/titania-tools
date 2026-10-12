//! Join: put files end to end, in order, without touching what is in them.

use crate::error::Result;
use crate::output::{Output, check_output};
use crate::progress::{Meter, Sink, games};
use crate::scan::{Games, separator, trim_end};
use crate::source::{display, open_all};
use serde::Serialize;
use std::path::PathBuf;
use std::time::Instant;

pub struct JoinOptions {
    pub inputs: Vec<PathBuf>,
    pub output: PathBuf,
}

#[derive(Serialize)]
pub struct JoinResult {
    pub output: String,
    pub files: u64,
    pub games: u64,
    pub bytes_written: u64,
    pub elapsed_ms: u64,
}

pub fn join(options: &JoinOptions, sink: &mut dyn Sink) -> Result<JoinResult> {
    let started = Instant::now();
    check_output(&options.output, &options.inputs)?;
    let sources = open_all(&options.inputs)?;
    let total: u64 = sources.iter().map(|s| s.bytes().len() as u64).sum();

    let mut output = Output::create(&options.output)?;
    let mut meter = Meter::new(sink);
    let mut done = 0u64;
    let mut total_games = 0u64;
    meter.start("Joining", total)?;
    for source in &sources {
        let data = trim_end(source.bytes());

        // The games are only counted; the file is copied exactly as it is.
        let mut count = 0u64;
        let mut scanner = Games::new(data);
        while scanner.next().is_some() {
            count += 1;
            meter.tick(done + scanner.position() as u64)?;
        }
        if !data.is_empty() {
            for chunk in data.chunks(8 << 20) {
                output.write(chunk)?;
                // Copying a large file takes a while; this is where a cancel gets noticed.
                meter.report(done + data.len() as u64)?;
            }
            output.write(separator(data))?;
        }
        done += source.bytes().len() as u64;
        total_games += count;
        meter.report(done)?;
        meter.log(&format!("{}: {}", source.name(), games(count)))?;
        meter.log_unclosed(&source.name(), scanner.unclosed_comments())?;
    }

    // Nothing may report to the sink from here on: once the file is saved it is too late to cancel.
    let bytes_written = output.commit()?;
    Ok(JoinResult {
        output: display(&options.output),
        files: sources.len() as u64,
        games: total_games,
        bytes_written,
        elapsed_ms: started.elapsed().as_millis() as u64,
    })
}
