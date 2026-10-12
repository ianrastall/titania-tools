//! PGN file tools for the Titania suite: report, join, merge, split and sort.
//!
//! Files are memory-mapped and handled as bytes, so games pass through untouched whatever their
//! encoding. Every operation takes a [`Sink`] for progress, log lines and cancellation.

mod combine;
mod error;
mod header;
mod join;
mod output;
mod progress;
mod report;
mod request;
mod scan;
mod source;
mod split;

pub use combine::{CombineOptions, CombineResult, Field, SortKey, combine};
pub use error::{Error, Result};
pub use join::{JoinOptions, JoinResult, join};
pub use progress::{NoSink, Sink};
pub use report::{Count, Report, report};
pub use request::{Request, run, run_json};
pub use scan::{Game, Games, MoveStats, Tags, walk_moves};
pub use split::{SplitMode, SplitOptions, SplitResult, split};
