//! Operations as JSON: the form the C ABI speaks.

use crate::combine::{CombineOptions, SortKey, combine};
use crate::error::{Result, message};
use crate::join::{JoinOptions, join};
use crate::progress::Sink;
use crate::report::report;
use crate::split::{SplitMode, SplitOptions, split};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

#[derive(Deserialize)]
#[serde(tag = "op", rename_all = "snake_case")]
pub enum Request {
    Report {
        input: PathBuf,
    },
    Join {
        inputs: Vec<PathBuf>,
        output: PathBuf,
    },
    Merge {
        inputs: Vec<PathBuf>,
        output: PathBuf,
        #[serde(default = "yes")]
        remove_duplicates: bool,
        #[serde(default)]
        keys: Vec<SortKey>,
    },
    Sort {
        input: PathBuf,
        output: PathBuf,
        keys: Vec<SortKey>,
    },
    Split {
        input: PathBuf,
        output_dir: PathBuf,
        mode: SplitMode,
        #[serde(default)]
        count: u64,
    },
}

fn yes() -> bool {
    true
}

/// Runs a request and returns its result as JSON.
pub fn run(request: Request, sink: &mut dyn Sink) -> Result<Vec<u8>> {
    match request {
        Request::Report { input } => to_json(&report(&input, sink)?),
        Request::Join { inputs, output } => to_json(&join(&JoinOptions { inputs, output }, sink)?),
        Request::Merge { inputs, output, remove_duplicates, keys } => {
            to_json(&combine(&CombineOptions { inputs, output, remove_duplicates, keys }, sink)?)
        }
        Request::Sort { input, output, keys } => {
            if keys.is_empty() {
                return Err(message("Choose what to sort by."));
            }
            to_json(&combine(&CombineOptions { inputs: vec![input], output, remove_duplicates: false, keys }, sink)?)
        }
        Request::Split { input, output_dir, mode, count } => to_json(&split(&SplitOptions { input, output_dir, mode, count }, sink)?),
    }
}

/// Parses a JSON request and runs it. The outer error means the request itself could not be read.
pub fn run_json(request: &[u8], sink: &mut dyn Sink) -> std::result::Result<Result<Vec<u8>>, String> {
    let request: Request = serde_json::from_slice(request).map_err(|e| format!("Bad request: {e}"))?;
    Ok(run(request, sink))
}

fn to_json<T: Serialize>(value: &T) -> Result<Vec<u8>> {
    serde_json::to_vec(value).map_err(|e| message(format!("Could not write the result: {e}")))
}
