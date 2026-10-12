# Titania Tools

Chess tools for Windows, built on a shared native core. The first of them is **PGN Simple Edit**.

## PGN Simple Edit

A desktop app for working with PGN chess game files as whole files: see what is in them, and
join, merge, split and sort them. It does not edit individual games.

| Tool | What it does |
| --- | --- |
| **Info** | Reports on a file: number of games, results, date range and games per year, most common players, events and ECO codes, ratings, game lengths, encoding and line endings. |
| **Join** | Puts files end to end as one new file. Nothing inside them is changed. |
| **Merge** | Combines files into one, optionally leaving out duplicate games and sorting the result. |
| **Split** | Breaks a file into several: a set number of games per file, a set number of files, or one file per year, event, ECO code, result, White player or Black player. |
| **Sort** | Orders the games of a file by up to three of: date, event, site, round, players, result, ECO code, ratings and number of moves. |

Files are added with the file picker, by dropping them on the window, or by opening them with
the app from Explorer.

### How it treats your files

- Files are memory-mapped and handled as bytes, so games pass through untouched whatever their
  encoding. Multi-gigabyte files are fine; memory use follows the number of games, not file size.
- Output is written under a temporary name and only takes its real name once it is complete.
  A failed or cancelled run leaves nothing behind and never destroys an existing file. A split
  replaces existing files only at the very end, and puts them back if that fails.
- An output file may not be one of the files being read.
- While a file is being read, other programs can read it but not change it.
- Two games are duplicates when they have the same players, date, starting position, variant
  and main-line moves. Comments, variations and annotations do not count.
- The reader is forgiving of real-world PGN: missing blank lines, several tags on one line,
  games without tags, unescaped quotes, comments that run over many lines or never close, and
  the Ctrl-Z mark old DOS tools leave at the end of a file.

## Requirements

To run: Windows 10 version 1809 or later, 64-bit. The installer carries the .NET and Windows App
SDK runtimes with it.

To build:

- [.NET 10 SDK](https://dotnet.microsoft.com/download)
- [Rust](https://rustup.rs) (edition 2024)
- PowerShell 7 (`pwsh`), which the build uses to compile the core
- [Inno Setup 6](https://jrsoftware.org/isdl.php), only for the installer

## Building

Build and run the app. This also builds the native core with Cargo:

```
dotnet run --project apps/PgnSimpleEdit
```

Run the core's tests:

```
cargo test --release --manifest-path core/Cargo.toml
```

Build the installer, which is written to `artifacts\installer`:

```
pwsh -File build-installer.ps1
```

## Layout

```
apps/PgnSimpleEdit    The app: C#, WinUI 3
core/titania-pgn      The PGN tools: Rust
core/titania-ffi      The C ABI over them, built as titania_core.dll
core/include          titania_core.h, the contract of that DLL
installer             Inno Setup script
tools                 Helper scripts
```

## The core

The core can be used without the app. `titania_core.dll` has one entry point, `tt_run`, which
takes a JSON request and returns a JSON result, with progress, log lines and cancellation going
through a callback. The requests are listed in
[`core/include/titania_core.h`](core/include/titania_core.h).

To try a request from a terminal:

```
cd core
cargo run --release --example tt -- "{\"op\":\"report\",\"input\":\"games.pgn\"}"
```

## License

[MIT](LICENSE)
