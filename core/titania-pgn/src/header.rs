//! The handful of tags the tools care about, and helpers for reading their values.

use crate::scan::{Tags, trim_end, trim_start};
use std::borrow::Cow;

pub(crate) type Value<'a> = Option<Cow<'a, [u8]>>;

#[derive(Default)]
pub(crate) struct Header<'a> {
    pub event: Value<'a>,
    pub site: Value<'a>,
    pub date: Value<'a>,
    pub utc_date: Value<'a>,
    pub round: Value<'a>,
    pub white: Value<'a>,
    pub black: Value<'a>,
    pub result: Value<'a>,
    pub eco: Value<'a>,
    pub white_elo: Value<'a>,
    pub black_elo: Value<'a>,
    pub fen: Value<'a>,
    pub variant: Value<'a>,
}

impl<'a> Header<'a> {
    pub fn read(tags: &'a [u8]) -> Self {
        let mut header = Self::default();
        for (name, value) in Tags::new(tags) {
            let slot = match name {
                b"Event" => &mut header.event,
                b"Site" => &mut header.site,
                b"Date" => &mut header.date,
                b"UTCDate" => &mut header.utc_date,
                b"Round" => &mut header.round,
                b"White" => &mut header.white,
                b"Black" => &mut header.black,
                b"Result" => &mut header.result,
                b"ECO" => &mut header.eco,
                b"WhiteElo" => &mut header.white_elo,
                b"BlackElo" => &mut header.black_elo,
                b"FEN" => &mut header.fen,
                b"Variant" => &mut header.variant,
                _ => continue,
            };
            // The first occurrence of a repeated tag wins.
            if slot.is_none() {
                *slot = Some(value);
            }
        }
        header
    }

    /// The game's date: the Date tag, or UTCDate when Date has no year.
    pub fn date(&self) -> &[u8] {
        best_date(text(&self.date), text(&self.utc_date))
    }

    /// The position the game starts from; empty for the ordinary starting position.
    pub fn start_position(&self) -> &[u8] {
        let fen = trim(text(&self.fen));
        if fen == STANDARD_START { b"" } else { fen }
    }

    /// The chess variant; empty for ordinary chess.
    pub fn variant(&self) -> &[u8] {
        let variant = trim(text(&self.variant));
        let standard = [&b"standard"[..], b"chess", b"normal"].iter().any(|name| variant.eq_ignore_ascii_case(name));
        if standard { b"" } else { variant }
    }
}

const STANDARD_START: &[u8] = b"rnbqkbnr/pppppppp/8/8/8/8/PPPPPPPP/RNBQKBNR w KQkq - 0 1";

fn trim(value: &[u8]) -> &[u8] {
    trim_end(trim_start(value))
}

pub(crate) fn best_date<'d>(date: &'d [u8], utc_date: &'d [u8]) -> &'d [u8] {
    if year(date).is_none() && year(utc_date).is_some() { utc_date } else { date }
}

/// True for the ways PGN writes "not known".
pub(crate) fn is_unknown(value: &[u8]) -> bool {
    matches!(value, b"" | b"?" | b"-")
}

/// A tag's value, or an empty slice when the tag is missing or unknown.
pub(crate) fn text<'v>(value: &'v Value<'_>) -> &'v [u8] {
    match value {
        Some(v) if !is_unknown(v) => v,
        _ => b"",
    }
}

/// A tag's value as a number; zero when it is missing or not a number.
pub(crate) fn number(value: &[u8]) -> u32 {
    let digits = value.iter().take_while(|b| b.is_ascii_digit()).count();
    if digits == 0 || digits > 9 {
        return 0;
    }
    value[..digits].iter().fold(0, |n, b| n * 10 + u32::from(b - b'0'))
}

/// A rating from a tag, or zero when it is not a believable one.
pub(crate) fn elo(value: &[u8]) -> u32 {
    match number(value) {
        n @ 1..=4000 => n,
        _ => 0,
    }
}

/// The year of a "YYYY.MM.DD" date, if it has one.
pub(crate) fn year(date: &[u8]) -> Option<u16> {
    let digits = date.get(..4)?;
    if !digits.iter().all(u8::is_ascii_digit) || date.get(4).is_some_and(u8::is_ascii_digit) {
        return None;
    }
    match number(digits) {
        0 => None,
        n => Some(n as u16),
    }
}

/// Text for display: UTF-8 when it is valid, otherwise read as Latin-1, which is what older PGN
/// files that are not UTF-8 almost always are.
pub(crate) fn decode(bytes: &[u8]) -> String {
    match std::str::from_utf8(bytes) {
        Ok(text) => text.to_string(),
        Err(_) => bytes.iter().map(|&b| b as char).collect(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_years() {
        assert_eq!(year(b"1998.07.14"), Some(1998));
        assert_eq!(year(b"1998.??.??"), Some(1998));
        assert_eq!(year(b"1998"), Some(1998));
        assert_eq!(year(b"????.??.??"), None);
        assert_eq!(year(b"19981231"), None);
        assert_eq!(year(b""), None);
    }

    #[test]
    fn falls_back_to_utc_date() {
        let header = Header::read(b"[Date \"????.??.??\"]\n[UTCDate \"2021.03.04\"]\n");
        assert_eq!(header.date(), b"2021.03.04");
    }

    #[test]
    fn the_standard_start_is_no_position() {
        let header = Header::read(b"[FEN \"rnbqkbnr/pppppppp/8/8/8/8/PPPPPPPP/RNBQKBNR w KQkq - 0 1 \"]\n[Variant \"Standard\"]\n");
        assert_eq!((header.start_position(), header.variant()), (&b""[..], &b""[..]));
        let header = Header::read(b"[FEN \"8/8/8/8/8/5k2/8/5K1R w - - 0 1\"]\n[Variant \"Chess960\"]\n");
        assert_eq!((header.start_position(), header.variant()), (&b"8/8/8/8/8/5k2/8/5K1R w - - 0 1"[..], &b"Chess960"[..]));
    }

    #[test]
    fn decodes_latin1() {
        assert_eq!(decode(b"Kr\xE4mer"), "Krämer");
        assert_eq!(decode("Krämer".as_bytes()), "Krämer");
    }
}
