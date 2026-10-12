//! Runs one JSON request against the core, for trying things out from a terminal:
//!
//!     cargo run --release --example tt -- "{\"op\":\"report\",\"input\":\"games.pgn\"}"

use std::io::Write;
use titania_pgn::{Sink, run_json};

struct Stderr;

impl Sink for Stderr {
    fn progress(&mut self, _done: u64, _total: u64, _games: u64) -> bool {
        true
    }
    fn phase(&mut self, text: &str) -> bool {
        eprintln!("-- {text}");
        true
    }
    fn log(&mut self, text: &str) -> bool {
        eprintln!("{text}");
        true
    }
}

fn main() {
    let request = std::env::args().nth(1).expect("a JSON request");
    match run_json(request.as_bytes(), &mut Stderr) {
        Ok(Ok(json)) => {
            let mut out = std::io::stdout().lock();
            out.write_all(&json).unwrap();
            out.write_all(b"\n").unwrap();
        }
        Ok(Err(error)) => {
            eprintln!("error: {error}");
            std::process::exit(2);
        }
        Err(error) => {
            eprintln!("{error}");
            std::process::exit(3);
        }
    }
}
