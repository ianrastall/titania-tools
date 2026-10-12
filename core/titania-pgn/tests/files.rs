//! The operations run end to end against real files: what gets written, replaced and left alone.

use std::fs;
use std::path::{Path, PathBuf};
use titania_pgn::{CombineOptions, Error, JoinOptions, NoSink, Sink, SplitMode, SplitOptions, combine, join, report, split};

/// A folder for one test, removed again when the test is over.
struct Folder(PathBuf);

impl Folder {
    fn new(test: &str) -> Self {
        let path = std::env::temp_dir().join(format!("titania-test-{}-{test}", std::process::id()));
        let _ = fs::remove_dir_all(&path);
        fs::create_dir_all(&path).unwrap();
        Self(path)
    }

    fn path(&self, name: &str) -> PathBuf {
        self.0.join(name)
    }

    fn write(&self, name: &str, text: &str) -> PathBuf {
        let path = self.path(name);
        fs::write(&path, text).unwrap();
        path
    }

    /// The names of everything in the folder, sorted.
    fn names(&self) -> Vec<String> {
        let mut names: Vec<_> = fs::read_dir(&self.0).unwrap().map(|entry| entry.unwrap().file_name().to_string_lossy().into_owned()).collect();
        names.sort();
        names
    }
}

impl Drop for Folder {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

/// A sink that runs a closure on every event; the closure returns `false` to cancel.
struct Events<F: FnMut(Event) -> bool>(F);

enum Event<'a> {
    Progress,
    Phase(&'a str),
    Log,
}

impl<F: FnMut(Event) -> bool> Sink for Events<F> {
    fn progress(&mut self, _done: u64, _total: u64, _games: u64) -> bool {
        (self.0)(Event::Progress)
    }
    fn phase(&mut self, text: &str) -> bool {
        (self.0)(Event::Phase(text))
    }
    fn log(&mut self, _text: &str) -> bool {
        (self.0)(Event::Log)
    }
}

fn game(event: &str, white: &str, moves: &str) -> String {
    format!("[Event \"{event}\"]\n[White \"{white}\"]\n[Black \"Bob\"]\n[Date \"2020.01.01\"]\n\n{moves}\n\n")
}

fn merge(inputs: &[&Path], output: &Path) -> titania_pgn::CombineResult {
    let options = CombineOptions { inputs: inputs.iter().map(|p| p.to_path_buf()).collect(), output: output.to_path_buf(), remove_duplicates: true, keys: vec![] };
    combine(&options, &mut NoSink).unwrap()
}

fn split_options(input: &Path, folder: &Folder, mode: SplitMode, count: u64) -> SplitOptions {
    SplitOptions { input: input.to_path_buf(), output_dir: folder.0.join("out"), mode, count }
}

fn count_games(path: &Path) -> u64 {
    report(path, &mut NoSink).unwrap().games
}

#[test]
fn a_cancelled_split_leaves_existing_files_alone() {
    let folder = Folder::new("split-cancel");
    let input = folder.write("games.pgn", &(game("A", "Alice", "1. e4 *") + &game("B", "Alice", "1. d4 *")));
    let out = Folder(folder.path("out"));
    fs::create_dir_all(&out.0).unwrap();
    out.write("games_001.pgn", "KEEP ME");

    // Cancel at the end of the splitting phase, when every game has been written.
    let (mut splitting, mut reports) = (false, 0);
    let mut sink = Events(|event| {
        match event {
            Event::Phase(text) => splitting = text == "Splitting",
            Event::Progress if splitting => reports += 1,
            _ => {}
        }
        reports < 2
    });
    let result = split(&split_options(&input, &folder, SplitMode::Games, 1), &mut sink);
    assert!(matches!(result, Err(Error::Cancelled)));
    assert_eq!(out.names(), ["games_001.pgn"]);
    assert_eq!(fs::read_to_string(out.path("games_001.pgn")).unwrap(), "KEEP ME");
    std::mem::forget(out);
}

#[test]
fn a_finished_split_replaces_existing_files_and_cleans_up() {
    let folder = Folder::new("split-replace");
    let input = folder.write("games.pgn", &(game("A", "Alice", "1. e4 *") + &game("B", "Alice", "1. d4 *")));
    let out = Folder(folder.path("out"));
    fs::create_dir_all(&out.0).unwrap();
    out.write("games_001.pgn", "OLD");
    out.write("other.pgn", "UNRELATED");

    let result = split(&split_options(&input, &folder, SplitMode::Games, 1), &mut NoSink).unwrap();
    assert_eq!((result.games, result.files), (2, 2));
    assert_eq!(out.names(), ["games_001.pgn", "games_002.pgn", "other.pgn"]);
    assert!(fs::read_to_string(out.path("games_001.pgn")).unwrap().starts_with("[Event \"A\"]"));
    assert_eq!(fs::read_to_string(out.path("other.pgn")).unwrap(), "UNRELATED");
    std::mem::forget(out);
}

#[test]
fn a_split_that_cannot_be_saved_puts_everything_back() {
    let folder = Folder::new("split-rollback");
    let input = folder.write("games.pgn", &(game("A", "Alice", "1. e4 *") + &game("B", "Alice", "1. d4 *")));
    let out = Folder(folder.path("out"));
    fs::create_dir_all(&out.0).unwrap();
    out.write("games_001.pgn", "OLD");
    // A folder with something in it stands where the second file has to go, and is locked in
    // place by the open file inside it.
    fs::create_dir_all(out.path("games_002.pgn")).unwrap();
    let blocker = out.path("games_002.pgn").join("inside.txt");
    fs::write(&blocker, "x").unwrap();
    let held = fs::File::open(&blocker).unwrap();

    let result = split(&split_options(&input, &folder, SplitMode::Games, 1), &mut NoSink);
    drop(held);
    if result.is_ok() {
        // This system let the folder be moved aside after all; there is nothing to roll back.
        std::mem::forget(out);
        return;
    }
    assert_eq!(out.names(), ["games_001.pgn", "games_002.pgn"]);
    assert_eq!(fs::read_to_string(out.path("games_001.pgn")).unwrap(), "OLD");
    assert!(blocker.exists());
    std::mem::forget(out);
}

#[test]
fn two_operations_cannot_save_to_the_same_file_at_once() {
    let folder = Folder::new("same-output");
    let a = folder.write("a.pgn", &game("A", "Alice", "1. e4 *"));
    let b = folder.write("b.pgn", &game("B", "Alice", "1. d4 *"));
    let output = folder.path("joined.pgn");

    // While the first join is running, a second one asks for the same output.
    let mut second = None;
    let mut sink = Events(|event| {
        if matches!(event, Event::Phase(_)) {
            second = Some(join(&JoinOptions { inputs: vec![b.clone()], output: output.clone() }, &mut NoSink));
        }
        true
    });
    join(&JoinOptions { inputs: vec![a.clone()], output: output.clone() }, &mut sink).unwrap();
    assert!(matches!(second, Some(Err(Error::Message(text))) if text.contains("another operation")));
    assert_eq!(fs::read_to_string(&output).unwrap().trim_end(), game("A", "Alice", "1. e4 *").trim_end());
    assert_eq!(folder.names(), ["a.pgn", "b.pgn", "joined.pgn"]);

    // Afterwards the output is free again.
    join(&JoinOptions { inputs: vec![b], output: output.clone() }, &mut NoSink).unwrap();
    assert!(fs::read_to_string(&output).unwrap().starts_with("[Event \"B\"]"));
}

#[test]
fn the_same_moves_from_another_position_are_not_a_duplicate() {
    let folder = Folder::new("fen");
    let tags = "[White \"Alice\"]\n[Black \"Bob\"]\n[Date \"2020.01.01\"]\n";
    let input = folder.write(
        "games.pgn",
        &format!(
            "[Event \"A\"]\n{tags}[FEN \"4k3/8/8/8/8/8/4P3/4K3 w - - 0 1\"]\n\n1. e4 *\n\n\
             [Event \"A\"]\n{tags}[FEN \"4k3/8/8/8/8/8/3PP3/4K3 w - - 0 1\"]\n\n1. e4 *\n\n\
             [Event \"A\"]\n{tags}\n1. e4 *\n\n\
             [Event \"A\"]\n{tags}[FEN \"rnbqkbnr/pppppppp/8/8/8/8/PPPPPPPP/RNBQKBNR w KQkq - 0 1\"]\n\n1. e4 *\n\n\
             [Event \"A\"]\n{tags}[Variant \"Atomic\"]\n\n1. e4 *\n"
        ),
    );
    let result = merge(&[&input], &folder.path("merged.pgn"));
    // Only the game that names the ordinary starting position repeats the one without a FEN tag.
    assert_eq!((result.games_read, result.games_written, result.duplicates_removed), (5, 4, 1));
}

#[test]
fn games_with_their_tags_on_one_line_keep_their_players() {
    let folder = Folder::new("compact");
    let input = folder.write(
        "games.pgn",
        "[Event \"A\"] [White \"Alice\"] [Black \"Bob\"]\n\n1. e4 *\n\n[Event \"A\"] [White \"Carol\"] [Black \"Dave\"]\n\n1. e4 *\n",
    );
    let result = merge(&[&input], &folder.path("merged.pgn"));
    assert_eq!((result.games_read, result.games_written), (2, 2));
    assert_eq!(report(&input, &mut NoSink).unwrap().players, 4);
}

#[test]
fn a_move_after_an_annotation_is_not_lost() {
    let folder = Folder::new("nag");
    let input = folder.write("games.pgn", &(game("A", "Alice", "1. e4 *") + &game("A", "Alice", "1. e4 $1e5 *") + &game("A", "Alice", "1. e4$1 *")));
    let result = merge(&[&input], &folder.path("merged.pgn"));
    // The second game has a move more; the third is the first with an annotation.
    assert_eq!((result.games_read, result.games_written), (3, 2));
}

#[test]
fn an_event_tag_in_a_comment_does_not_split_the_game() {
    let folder = Folder::new("comment");
    let input = folder.write("games.pgn", &game("A", "Alice", "1. e4 { compare\n[Event \"Another\"]\nwhere Black won } e5 1-0"));
    let found = report(&input, &mut NoSink).unwrap();
    assert_eq!((found.games, found.events, found.total_plies), (1, 1, 2));
}

#[cfg(windows)]
#[test]
fn an_input_cannot_be_changed_while_it_is_being_read() {
    let folder = Folder::new("locked");
    let input = folder.write("games.pgn", &game("A", "Alice", "1. e4 *"));
    let writable = |path: &Path| fs::OpenOptions::new().write(true).open(path).is_ok();

    let mut opened = false;
    let mut sink = Events(|_| {
        opened |= writable(&input);
        true
    });
    let options = CombineOptions { inputs: vec![input.clone()], output: folder.path("sorted.pgn"), remove_duplicates: false, keys: vec![] };
    combine(&options, &mut sink).unwrap();
    assert!(!opened);
    assert!(writable(&input));
}

#[test]
fn groups_with_the_same_file_name_get_files_of_their_own() {
    let folder = Folder::new("split-names");
    let input = folder.write("games.pgn", &(game("A/B", "Alice", "1. e4 *") + &game("A_B", "Alice", "1. d4 *") + &game("a/b", "Alice", "1. c4 *")));
    let result = split(&split_options(&input, &folder, SplitMode::Event, 0), &mut NoSink).unwrap();
    let list: Vec<_> = result.list.iter().map(|file| (file.name.as_str(), file.games)).collect();
    assert_eq!(list, [("games_A_B (2).pgn", 1), ("games_A_B.pgn", 2)]);
    assert_eq!(count_games(&folder.path("out").join("games_A_B.pgn")), 2);
}

#[test]
fn a_set_number_of_files_is_what_is_made() {
    let folder = Folder::new("split-parts");
    let ten: String = (0..10).map(|i| game(&format!("E{i}"), "Alice", "1. e4 *")).collect();
    let input = folder.write("games.pgn", &ten);
    let result = split(&split_options(&input, &folder, SplitMode::Parts, 6), &mut NoSink).unwrap();
    let sizes: Vec<_> = result.list.iter().map(|file| file.games).collect();
    assert_eq!(sizes, [2, 2, 2, 2, 1, 1]);

    // Fewer games than files: one game in each.
    let few = Folder::new("split-parts-few");
    let input = few.write("games.pgn", &ten[..game("E0", "Alice", "1. e4 *").len() * 3]);
    let result = split(&split_options(&input, &few, SplitMode::Parts, 5), &mut NoSink).unwrap();
    assert_eq!(result.list.iter().map(|file| file.games).collect::<Vec<_>>(), [1, 1, 1]);
}

#[test]
fn a_cancel_from_the_last_log_line_saves_nothing() {
    let folder = Folder::new("join-cancel");
    let input = folder.write("a.pgn", &game("A", "Alice", "1. e4 *"));
    let output = folder.path("joined.pgn");
    let mut sink = Events(|event| !matches!(event, Event::Log));
    let result = join(&JoinOptions { inputs: vec![input], output }, &mut sink);
    assert!(matches!(result, Err(Error::Cancelled)));
    assert_eq!(folder.names(), ["a.pgn"]);
}

#[test]
fn a_dos_end_of_file_mark_is_not_a_game() {
    let folder = Folder::new("eof");
    let input = folder.write("games.pgn", "[Event \"A\"]\n\n\x1a");
    assert_eq!(count_games(&input), 1);
    let input = folder.write("more.pgn", &(game("A", "Alice", "1. e4 *") + "\x1a"));
    assert_eq!(count_games(&input), 1);
}

#[test]
fn games_without_tags_end_at_their_results() {
    let folder = Folder::new("tagless");
    let input = folder.write("games.pgn", "1. e4 e5\n\n2. Nf3 1-0\n1. d4 d5 0-1\n\n1. c4 1/2-1/2 1. f4 *\n");
    let found = report(&input, &mut NoSink).unwrap();
    assert_eq!(found.games, 4);
    // The results are read from the moves when there is no Result tag.
    assert_eq!((found.white_wins, found.black_wins, found.draws, found.other_results), (1, 1, 1, 1));
}

#[test]
fn a_file_that_cannot_be_saved_leaves_nothing_behind() {
    let folder = Folder::new("commit");
    let input = folder.write("a.pgn", &game("A", "Alice", "1. e4 *"));
    // A folder with something in it stands where the output has to go.
    let output = folder.path("joined.pgn");
    fs::create_dir_all(&output).unwrap();
    fs::write(output.join("inside.txt"), "x").unwrap();

    let result = join(&JoinOptions { inputs: vec![input], output }, &mut NoSink);
    assert!(matches!(result, Err(Error::Message(_))));
    assert_eq!(folder.names(), ["a.pgn", "joined.pgn"]);
}
