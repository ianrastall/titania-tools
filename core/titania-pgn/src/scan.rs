//! Splitting PGN text into games, and reading tags and moves out of a game.
//!
//! The scanner is forgiving: real PGN files have stray text, missing blank lines, unescaped
//! quotes and comments that run over many lines.

use memchr::{memchr, memchr2};
use std::borrow::Cow;

/// One game, as slices of the text it was found in.
pub struct Game<'a> {
    /// Where `raw` starts in the scanned text.
    pub offset: usize,
    /// The whole game without trailing whitespace.
    pub raw: &'a [u8],
    /// The tag section. Empty for a game that has only moves.
    pub tags: &'a [u8],
    /// Everything after the tag section. May be empty.
    pub movetext: &'a [u8],
}

/// Iterates over the games in a PGN text.
pub struct Games<'a> {
    data: &'a [u8],
    pos: usize,
    unclosed: u64,
}

impl<'a> Games<'a> {
    pub fn new(data: &'a [u8]) -> Self {
        Self { data, pos: 0, unclosed: 0 }
    }

    /// How far into the text the scanner has read.
    pub fn position(&self) -> usize {
        self.pos
    }

    /// How many games so far ended inside a comment that was never closed, and were cut off
    /// where the next game starts.
    pub fn unclosed_comments(&self) -> u64 {
        self.unclosed
    }
}

impl<'a> Iterator for Games<'a> {
    type Item = Game<'a>;

    fn next(&mut self) -> Option<Game<'a>> {
        const NONE: usize = usize::MAX;
        let data = self.data;
        let len = data.len();
        let mut pos = self.pos;
        let mut start = NONE;
        let mut moves_at = NONE;
        let mut tagless = false;
        let mut seen_event = false;
        let mut blank_in_tags = false;
        // Inside a comment that is not closed properly.
        let mut in_brace = false;
        // How deep in variations the movetext is.
        let mut depth = 0u32;
        // The result that ends the game has been read; only comments may still follow.
        let mut ended = false;
        let mut end = len;

        // `pos` is the start of a line, or the place in a line where the tags or a comment ended.
        'scan: while pos < len {
            let eol = memchr(b'\n', &data[pos..]).map_or(len, |i| pos + i);
            let next_line = (eol + 1).min(len);
            let mut at = pos + spaces(&data[pos..eol]);
            let first = (at < eol).then(|| data[at]);

            if moves_at == NONE {
                // Looking for the start of a game, or in its tag section.
                match first {
                    None => blank_in_tags = start != NONE,
                    Some(b'%') => {}
                    Some(b'[') => loop {
                        // A line may hold several tags, and the moves may follow on the same line.
                        let tag = &data[at..eol];
                        if is_event_tag(tag) {
                            // A second Event tag, or one after a blank line: the previous game had no moves.
                            if start != NONE && (seen_event || blank_in_tags) {
                                end = at;
                                break 'scan;
                            }
                            seen_event = true;
                        }
                        if start == NONE {
                            start = at;
                        }
                        at += read_tag(tag).2;
                        at += spaces(&data[at..eol]);
                        if at < eol && data[at] != b'[' {
                            moves_at = at;
                        }
                        if at >= eol || moves_at != NONE {
                            break;
                        }
                    },
                    Some(_) => {
                        if start == NONE {
                            start = at;
                            tagless = true;
                        }
                        moves_at = at;
                    }
                }
                if moves_at == NONE {
                    pos = next_line;
                    continue;
                }
            } else if in_brace {
                // Inside a comment that was never closed, where a line may start with '['. An
                // Event tag ends the game, so that the comment cannot swallow the rest of the file.
                if first == Some(b'[') && is_event_tag(&data[at..eol]) {
                    end = at;
                    break;
                }
                match memchr(b'}', &data[pos..eol]) {
                    Some(i) => {
                        in_brace = false;
                        at = pos + i + 1;
                    }
                    None => {
                        pos = next_line;
                        continue;
                    }
                }
            } else {
                match first {
                    Some(b'[') => {
                        end = at;
                        break;
                    }
                    Some(b'%') if pos == 0 || data[pos - 1] == b'\n' => at = eol,
                    _ => {}
                }
            }

            // Movetext from `at` to the end of the line. The game ends with its result, not at a
            // blank line; whatever follows the result, other than a comment, is the next game.
            let mut i = at;
            while i < eol {
                match data[i] {
                    b if is_space(&b) => i += 1,
                    b'{' => {
                        if let Some(close) = memchr(b'}', &data[i + 1..eol]) {
                            i += close + 2;
                            continue;
                        }
                        // The comment runs over the line. It is a proper one when it closes
                        // before another comment opens; then nothing inside it is looked at.
                        match memchr2(b'{', b'}', &data[eol..]) {
                            Some(close) if data[eol + close] == b'}' => {
                                pos = eol + close + 1;
                                continue 'scan;
                            }
                            _ => in_brace = true,
                        }
                        break;
                    }
                    // A ';' comment runs to the end of the line.
                    b';' => break,
                    b')' | b'}' => {
                        depth = depth.saturating_sub(u32::from(data[i] == b')'));
                        i += 1;
                    }
                    _ if ended => {
                        end = i;
                        break 'scan;
                    }
                    b'(' => {
                        depth += 1;
                        i += 1;
                    }
                    _ => {
                        let token = i + data[i..eol].iter().position(is_token_end).unwrap_or(eol - i);
                        ended = depth == 0 && result_of(&data[i..token]).is_some();
                        i = token;
                    }
                }
            }
            pos = next_line;
        }

        self.pos = end;
        if start == NONE {
            return None;
        }
        self.unclosed += u64::from(in_brace);
        let raw = trim_end(&data[start..end]);
        let stop = start + raw.len();
        let moves_at = moves_at.min(stop);
        Some(Game { offset: start, raw, tags: if tagless { &[] } else { &data[start..moves_at] }, movetext: &data[moves_at..stop] })
    }
}

/// How many bytes of whitespace `bytes` starts with.
fn spaces(bytes: &[u8]) -> usize {
    bytes.iter().position(|b| !is_space(b)).unwrap_or(bytes.len())
}

fn is_token_end(b: &u8) -> bool {
    is_space(b) || matches!(b, b'{' | b'}' | b'(' | b')' | b';')
}

/// The result a movetext token stands for, if it is one.
fn result_of(token: &[u8]) -> Option<&'static [u8]> {
    match token {
        b"1-0" => Some(b"1-0"),
        b"0-1" => Some(b"0-1"),
        b"1/2-1/2" => Some(b"1/2-1/2"),
        b"*" => Some(b"*"),
        _ => None,
    }
}

/// `tag` starts with '['.
fn is_event_tag(tag: &[u8]) -> bool {
    match trim_start(&tag[1..]).strip_prefix(b"Event") {
        Some(rest) => matches!(rest.first(), Some(b' ' | b'\t' | b'"')),
        None => false,
    }
}

/// Reads the tag that `line` starts with: its name, its value with escapes still in place, and
/// how much of the line it takes up. `line` starts with '[' and is at most one line long.
fn read_tag(line: &[u8]) -> (&[u8], &[u8], usize) {
    let name_at = 1 + spaces(&line[1..]);
    let name_len = line[name_at..].iter().position(|b| is_space(b) || matches!(b, b'"' | b'[' | b']')).unwrap_or(line.len() - name_at);
    let name = &line[name_at..name_at + name_len];
    let at = name_at + name_len + spaces(&line[name_at + name_len..]);

    if line.get(at) != Some(&b'"') {
        // No quotes: the value is whatever comes before the closing bracket.
        let close = memchr(b']', &line[at..]).map_or(line.len(), |i| at + i);
        return (name, trim_end(&line[at..close]), (close + 1).min(line.len()));
    }
    // The value ends at a quote that is followed by the closing bracket, so that a value with
    // unescaped quotes inside it, like [Event "The "Big" Open"], still reads whole.
    let from = at + 1;
    let mut i = from;
    while i < line.len() {
        match line[i] {
            b'\\' if matches!(line.get(i + 1), Some(b'"' | b'\\')) => i += 2,
            b'"' => {
                let after = i + 1 + spaces(&line[i + 1..]);
                if after == line.len() || line[after] == b']' {
                    return (name, &line[from..i], (after + 1).min(line.len()));
                }
                i += 1;
            }
            _ => i += 1,
        }
    }
    // No closing quote: the rest of the line is the value.
    let mut value = trim_end(&line[from..]);
    value = value.strip_suffix(b"]").map_or(value, trim_end);
    (name, value.strip_suffix(b"\"").unwrap_or(value), line.len())
}

/// Iterates over the `[Name "Value"]` pairs of a tag section.
pub struct Tags<'a> {
    data: &'a [u8],
    pos: usize,
}

impl<'a> Tags<'a> {
    pub fn new(tags: &'a [u8]) -> Self {
        Self { data: tags, pos: 0 }
    }
}

impl<'a> Iterator for Tags<'a> {
    type Item = (&'a [u8], Cow<'a, [u8]>);

    fn next(&mut self) -> Option<Self::Item> {
        let data = self.data;
        loop {
            self.pos += spaces(&data[self.pos..]);
            if self.pos >= data.len() {
                return None;
            }
            let eol = memchr(b'\n', &data[self.pos..]).map_or(data.len(), |i| self.pos + i);
            if data[self.pos] != b'[' {
                // Not a tag; skip the rest of the line.
                self.pos = eol;
                continue;
            }
            let (name, value, len) = read_tag(&data[self.pos..eol]);
            self.pos += len;
            if name.is_empty() {
                continue;
            }
            let value = if memchr(b'\\', value).is_some() { Cow::Owned(unescape(value)) } else { Cow::Borrowed(value) };
            return Some((name, value));
        }
    }
}

fn unescape(value: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(value.len());
    let mut i = 0;
    while i < value.len() {
        if value[i] == b'\\' && matches!(value.get(i + 1), Some(b'"' | b'\\')) {
            i += 1;
        }
        out.push(value[i]);
        i += 1;
    }
    out
}

/// What [`walk_moves`] found in a game's movetext.
#[derive(Default, Clone, Copy)]
pub struct MoveStats {
    /// Half-moves on the main line.
    pub plies: u32,
    pub has_comments: bool,
    pub has_variations: bool,
    /// The result the main line ends with, if it has one.
    pub result: Option<&'static [u8]>,
}

/// Walks the main line of a game, calling `on_move` with each move stripped of its move number
/// and of trailing check and annotation marks ("12.Nf3+!" becomes "Nf3").
pub fn walk_moves(text: &[u8], mut on_move: impl FnMut(&[u8])) -> MoveStats {
    let mut stats = MoveStats::default();
    let mut depth = 0u32;
    let len = text.len();
    let mut i = 0;
    while i < len {
        match text[i] {
            b'}' => i += 1,
            b if is_space(&b) => i += 1,
            b'{' => {
                stats.has_comments = true;
                i = memchr(b'}', &text[i..]).map_or(len, |j| i + j + 1);
            }
            b';' => {
                stats.has_comments = true;
                i = memchr(b'\n', &text[i..]).map_or(len, |j| i + j + 1);
            }
            b'%' if i == 0 || text[i - 1] == b'\n' => {
                i = memchr(b'\n', &text[i..]).map_or(len, |j| i + j + 1);
            }
            b'(' => {
                stats.has_variations = true;
                depth += 1;
                i += 1;
            }
            b')' => {
                depth = depth.saturating_sub(1);
                i += 1;
            }
            // A numeric annotation, "$14", ends where its digits end.
            b'$' => {
                i += 1;
                while i < len && text[i].is_ascii_digit() {
                    i += 1;
                }
            }
            _ => {
                let start = i;
                while i < len && text[i] != b'$' && !is_token_end(&text[i]) {
                    i += 1;
                }
                if depth == 0 {
                    let token = &text[start..i];
                    if let Some(mv) = as_move(token) {
                        stats.plies += 1;
                        on_move(mv);
                    } else if let Some(result) = result_of(token) {
                        stats.result = Some(result);
                    }
                }
            }
        }
    }
    stats
}

/// The move in a movetext token, if it is one: not a move number, result or annotation.
fn as_move(token: &[u8]) -> Option<&[u8]> {
    if result_of(token).is_some() || matches!(token, b"1/2" | b"e.p.") {
        return None;
    }
    // "12.Nf3" and "12...Nf6" carry a move number; "0-0" starts with a digit but is a move.
    let digits = token.iter().take_while(|b| b.is_ascii_digit()).count();
    let mut rest = token;
    if digits > 0 && token.get(digits) == Some(&b'.') {
        rest = &token[digits..];
        while let [b'.', tail @ ..] = rest {
            rest = tail;
        }
    }
    let first = *rest.first()?;
    if !(first.is_ascii_alphabetic() || rest.starts_with(b"0-0") || rest.starts_with(b"--")) {
        return None;
    }
    let end = rest.iter().rposition(|b| !matches!(b, b'+' | b'#' | b'!' | b'?')).map_or(0, |p| p + 1);
    (end > 0).then(|| &rest[..end])
}

fn is_space(b: &u8) -> bool {
    // 0x1A is the Ctrl-Z end-of-file mark that old DOS tools leave behind.
    matches!(b, b' ' | b'\t' | b'\r' | b'\n' | 0x1A)
}

pub(crate) fn trim_start(bytes: &[u8]) -> &[u8] {
    let start = bytes.iter().position(|b| !is_space(b)).unwrap_or(bytes.len());
    &bytes[start..]
}

pub(crate) fn trim_end(bytes: &[u8]) -> &[u8] {
    let end = bytes.iter().rposition(|b| !is_space(b)).map_or(0, |p| p + 1);
    &bytes[..end]
}

/// The line break to put after a game: the kind the game itself uses.
pub(crate) fn separator(game: &[u8]) -> &'static [u8] {
    match memchr(b'\n', game) {
        Some(i) if i > 0 && game[i - 1] == b'\r' => b"\r\n\r\n",
        _ => b"\n\n",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn games(text: &str) -> Vec<Game<'_>> {
        Games::new(text.as_bytes()).collect()
    }

    #[test]
    fn splits_ordinary_games() {
        let text = "[Event \"A\"]\n[White \"X\"]\n\n1. e4 e5 1-0\n\n[Event \"B\"]\n\n1. d4 *\n";
        let found = games(text);
        assert_eq!(found.len(), 2);
        assert_eq!(found[0].raw, b"[Event \"A\"]\n[White \"X\"]\n\n1. e4 e5 1-0");
        assert_eq!(found[0].movetext, b"1. e4 e5 1-0");
        assert_eq!(found[1].offset, text.find("[Event \"B\"]").unwrap());
    }

    #[test]
    fn handles_crlf_and_missing_blank_lines() {
        let found = games("[Event \"A\"]\r\n1. e4 *\r\n[Event \"B\"]\r\n1. d4 *\r\n");
        assert_eq!(found.len(), 2);
        assert_eq!(found[0].movetext, b"1. e4 *");
    }

    #[test]
    fn keeps_bracket_lines_inside_comments() {
        let found = games("[Event \"A\"]\n\n1. e4 { a note\n[%eval 0.3] still the note } e5 *\n\n[Event \"B\"]\n\n*\n");
        assert_eq!(found.len(), 2);
        assert!(found[0].raw.ends_with(b"e5 *"));
    }

    #[test]
    fn recovers_from_an_unclosed_comment() {
        assert_eq!(games("[Event \"A\"]\n\n1. e4 { oops\n\n[Event \"B\"]\n\n1. d4 *\n").len(), 2);
    }

    #[test]
    fn splits_games_without_moves() {
        let found = games("[Event \"A\"]\n[Result \"*\"]\n\n[Event \"B\"]\n[Result \"*\"]\n[Event \"C\"]\n");
        assert_eq!(found.len(), 3);
        assert!(found[0].movetext.is_empty());
    }

    #[test]
    fn splits_games_without_tags() {
        let found = games("1. e4 e5 *\n\n1. d4 d5 *\n\n[Event \"A\"]\n\n1. c4 *\n");
        assert_eq!(found.len(), 3);
        assert!(found[0].tags.is_empty());
        assert_eq!(found[1].movetext, b"1. d4 d5 *");
    }

    #[test]
    fn empty_and_blank_text_have_no_games() {
        assert!(games("").is_empty());
        assert!(games("\n \r\n\n").is_empty());
    }

    #[test]
    fn reads_tags() {
        let tags: Vec<_> = Tags::new(b"[Event \"The \"Big\" Open\"]\r\n[White \"O\\\"Neil, \\\\ J\"]\n[Round \"\"]\n[Odd value]\n").collect();
        assert_eq!(tags.len(), 4);
        assert_eq!((tags[0].0, &*tags[0].1), (&b"Event"[..], &b"The \"Big\" Open"[..]));
        assert_eq!(&*tags[1].1, b"O\"Neil, \\ J");
        assert_eq!(&*tags[2].1, b"");
        assert_eq!((tags[3].0, &*tags[3].1), (&b"Odd"[..], &b"value"[..]));
    }

    #[test]
    fn walks_the_main_line() {
        let mut moves = Vec::new();
        let text = b"1. e4! { best } e5 2.Nf3 (2. f4 exf4) 2... Nc6 $1 3. 0-0+ O-O-O# ; done\n4. -- 1/2-1/2";
        let stats = walk_moves(text, |m| moves.push(String::from_utf8_lossy(m).into_owned()));
        assert_eq!(moves, ["e4", "e5", "Nf3", "Nc6", "0-0", "O-O-O", "--"]);
        assert_eq!(stats.plies, 7);
        assert!(stats.has_comments && stats.has_variations);
    }

    #[test]
    fn results_are_not_moves() {
        for text in ["1-0", "0-1", "1/2-1/2", "*", "12.", "3...", "$14"] {
            assert_eq!(walk_moves(text.as_bytes(), |_| {}).plies, 0, "{text}");
        }
    }

    #[test]
    fn reads_several_tags_on_one_line() {
        let found = games("[Event \"A\"] [White \"Alice\"] [Black \"Bob\"] 1. e4 *
[Event \"B\"][White \"Carol\"]
1. d4 *
");
        assert_eq!(found.len(), 2);
        assert_eq!(found[0].movetext, b"1. e4 *");
        let tags: Vec<_> = Tags::new(found[0].tags).map(|(name, value)| (name, value.into_owned())).collect();
        assert_eq!(tags, [(&b"Event"[..], b"A".to_vec()), (&b"White"[..], b"Alice".to_vec()), (&b"Black"[..], b"Bob".to_vec())]);
        assert_eq!(Tags::new(found[1].tags).count(), 2);
    }

    #[test]
    fn an_event_tag_inside_a_closed_comment_is_not_a_game() {
        let scanner = &mut Games::new(b"[Event \"A\"]

1. e4 { see
[Event \"B\"]
for more } e5 *

[Event \"C\"]

*
");
        let found: Vec<_> = scanner.collect();
        assert_eq!(found.len(), 2);
        assert!(found[0].raw.ends_with(b"e5 *"));
        assert_eq!(scanner.unclosed_comments(), 0);
    }

    #[test]
    fn counts_unclosed_comments() {
        let scanner = &mut Games::new(b"[Event \"A\"]

1. e4 { oops

[Event \"B\"]

1. d4 { fine } *
");
        assert_eq!(scanner.count(), 2);
        assert_eq!(scanner.unclosed_comments(), 1);
    }

    #[test]
    fn a_result_ends_a_game_and_a_blank_line_does_not() {
        let found = games("1. e4 e5

2. Nf3 Nc6 1-0
1. d4 d5 0-1 1. c4 *
");
        assert_eq!(found.len(), 3);
        assert_eq!(found[0].raw, b"1. e4 e5

2. Nf3 Nc6 1-0");
        assert_eq!(found[1].raw, b"1. d4 d5 0-1");
        assert_eq!(found[2].raw, b"1. c4 *");
    }

    #[test]
    fn results_in_variations_and_comments_do_not_end_a_game() {
        let found = games("[Event \"A\"]

1. e4 (1. d4 1-0) { 0-1 } e5 ; *
2. Nf3 1/2-1/2 { agreed }
; the end

[Event \"B\"]

*
");
        assert_eq!(found.len(), 2);
        assert!(found[0].raw.ends_with(b"; the end"));
    }

    #[test]
    fn survives_a_dos_end_of_file_mark() {
        let found = games("[Event \"A\"]

");
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].raw, b"[Event \"A\"]");
        assert!(found[0].movetext.is_empty());
        assert!(games("
").is_empty());
        assert_eq!(games("1. e4 *
")[0].raw, b"1. e4 *");
    }

    #[test]
    fn annotations_end_at_their_last_digit() {
        let moves = |text: &str| {
            let mut moves = Vec::new();
            walk_moves(text.as_bytes(), |m| moves.push(String::from_utf8_lossy(m).into_owned()));
            moves
        };
        assert_eq!(moves("1. e4$1 e5 *"), ["e4", "e5"]);
        assert_eq!(moves("1. e4 $1e5 *"), ["e4", "e5"]);
        assert_eq!(moves("1. e4 $1$14 e5$2*"), ["e4", "e5"]);
    }

    #[test]
    fn finds_the_result_in_the_movetext() {
        assert_eq!(walk_moves(b"1. e4 e5 (1... c5 0-1) 1-0", |_| {}).result, Some(&b"1-0"[..]));
        assert_eq!(walk_moves(b"1. e4 e5 { 1-0 }", |_| {}).result, None);
    }
}
