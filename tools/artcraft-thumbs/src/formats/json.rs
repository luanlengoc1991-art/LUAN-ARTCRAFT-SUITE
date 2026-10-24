//! Documents saved as JSON (maybe gzip) with `"preview": {"mime": "image/png", "data":
//! "<base64>"}` near the start:
//!
//! - VectorCraft `.vectorcraft` / `.drawcraft`: after `format` (`vectorcraft` or `drawcraft`),
//!   `version` and `generator` (`apps/vectorcraft/crates/format/src/lib.rs`);
//! - FilmCraft `.fcproj`: after `format` (`filmcraft.project`), `schema_version` and `generator`
//!   (`apps/filmcraft/crates/format/src/lib.rs`);
//! - EffectCraft `.ecproj`: the first member, followed by `savedBy`
//!   (`apps/effectcraft/crates/engine/src/commands/file.rs`).

use std::io::Read;

use super::{App, Source, SourceReader, find};

/// Bytes of the head searched for the preview member.
const HEAD: usize = 4 << 10;
/// Most bytes read for a preview (its base64 included).
const MAX: usize = 48 << 20;
/// Bytes after the preview searched for EffectCraft's `savedBy`.
const AFTER: usize = 1 << 10;

/// The app and preview PNG of a JSON document (`gz`: gzip-compressed).
pub fn preview(src: &mut dyn Source, gz: bool) -> Option<(App, Vec<u8>)> {
    let len = src.size()?;
    let reader = SourceReader { src, at: 0, len };
    let mut text: Box<dyn Read + '_> = if gz {
        Box::new(flate2::read::GzDecoder::new(reader))
    } else {
        Box::new(reader)
    };
    let mut buf = Vec::new();
    let mut chunk = vec![0u8; 64 << 10];
    let mut fill = |buf: &mut Vec<u8>, want: usize| -> Option<()> {
        while buf.len() < want {
            let n = text.read(&mut chunk).ok()?;
            if n == 0 {
                break;
            }
            buf.extend_from_slice(chunk.get(..n)?);
        }
        Some(())
    };
    fill(&mut buf, HEAD)?;
    let head = buf.get(..HEAD.min(buf.len()))?;
    if head.iter().find(|b| !b.is_ascii_whitespace()) != Some(&b'{') {
        return None;
    }
    let marked =
        if find(head, b"\"vectorcraft\"").is_some() || find(head, b"\"drawcraft\"").is_some() {
            Some(App::VectorCraft)
        } else if find(head, b"\"filmcraft.project\"").is_some() {
            Some(App::FilmCraft)
        } else {
            None
        };
    let at = find(head, b"\"preview\"")?;
    // The envelope's member is an object; a layer's `"preview": true` is not it.
    let mut o = at + 9;
    while buf
        .get(o)
        .is_some_and(|b| b.is_ascii_whitespace() || *b == b':')
    {
        o += 1;
    }
    if buf.get(o) != Some(&b'{') {
        return None;
    }
    let data = o + find(buf.get(o..buf.len().min(o + 256))?, b"\"data\"")? + 6;
    let mut i = data;
    while buf
        .get(i)
        .is_some_and(|b| b.is_ascii_whitespace() || *b == b':')
    {
        i += 1;
    }
    if buf.get(i) != Some(&b'"') {
        return None;
    }
    let start = i + 1;
    let end = loop {
        if let Some(e) = buf
            .get(start..)
            .and_then(|b| b.iter().position(|&c| c == b'"'))
        {
            break start + e;
        }
        if buf.len() > MAX {
            return None;
        }
        let before = buf.len();
        fill(&mut buf, before + (1 << 20))?;
        if buf.len() == before {
            return None;
        }
    };
    let app = match marked {
        Some(app) => app,
        None => {
            fill(&mut buf, end + AFTER)?;
            find(buf.get(end..buf.len().min(end + AFTER))?, b"\"savedBy\"")
                .map(|_| App::EffectCraft)?
        }
    };
    base64_decode(buf.get(start..end)?).map(|png| (app, png))
}

fn base64_decode(s: &[u8]) -> Option<Vec<u8>> {
    let val = |c: u8| -> Option<u32> {
        Some(match c {
            b'A'..=b'Z' => c - b'A',
            b'a'..=b'z' => c - b'a' + 26,
            b'0'..=b'9' => c - b'0' + 52,
            b'+' => 62,
            b'/' => 63,
            _ => return None,
        } as u32)
    };
    let clean: Vec<u8> = s
        .iter()
        .copied()
        .filter(|b| !b.is_ascii_whitespace() && *b != b'\\')
        .collect();
    if !clean.len().is_multiple_of(4) {
        return None;
    }
    let mut out = Vec::with_capacity(clean.len() / 4 * 3);
    for c in clean.chunks(4) {
        let mut n = 0u32;
        let mut pad = 0;
        for &b in c {
            n <<= 6;
            if b == b'=' {
                pad += 1;
            } else {
                n |= val(b)?;
            }
        }
        out.push((n >> 16) as u8);
        if pad < 2 {
            out.push((n >> 8) as u8);
        }
        if pad < 1 {
            out.push(n as u8);
        }
    }
    Some(out)
}

#[cfg(test)]
pub(crate) fn b64(data: &[u8]) -> String {
    const B: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut s = String::new();
    for c in data.chunks(3) {
        let n = (c[0] as u32) << 16
            | (*c.get(1).unwrap_or(&0) as u32) << 8
            | *c.get(2).unwrap_or(&0) as u32;
        s.push(B[(n >> 18) as usize & 63] as char);
        s.push(B[(n >> 12) as usize & 63] as char);
        s.push(if c.len() > 1 {
            B[(n >> 6) as usize & 63] as char
        } else {
            '='
        });
        s.push(if c.len() > 2 {
            B[n as usize & 63] as char
        } else {
            '='
        });
    }
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    const PNG: &[u8] = b"\x89PNG fake";

    fn read(bytes: &[u8]) -> Option<(App, Vec<u8>)> {
        let mut src = bytes;
        preview(&mut src, bytes.starts_with(&[0x1f, 0x8b]))
    }

    #[test]
    fn vectorcraft_plain_pretty_and_gzip() {
        let compact = format!(
            r#"{{"format":"vectorcraft","version":4,"generator":"VectorCraft 0.11.0","preview":{{"mime":"image/png","data":"{}"}},"document":{{}}}}"#,
            b64(PNG)
        );
        assert_eq!(
            read(compact.as_bytes()),
            Some((App::VectorCraft, PNG.to_vec()))
        );
        let pretty = format!(
            "{{\n  \"format\": \"drawcraft\",\n  \"version\": 2,\n  \"preview\": {{\n    \"mime\": \"image/png\",\n    \"data\": \"{}\"\n  }}\n}}",
            b64(PNG)
        );
        assert_eq!(
            read(pretty.as_bytes()),
            Some((App::VectorCraft, PNG.to_vec()))
        );
        let mut gz = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::default());
        std::io::Write::write_all(&mut gz, compact.as_bytes()).unwrap();
        assert_eq!(
            read(&gz.finish().unwrap()),
            Some((App::VectorCraft, PNG.to_vec()))
        );
    }

    #[test]
    fn filmcraft_and_effectcraft() {
        let fc = format!(
            r#"{{"format":"filmcraft.project","schema_version":15,"generator":"FilmCraft 0.1.0","preview":{{"mime":"image/png","data":"{}"}},"project":{{}}}}"#,
            b64(PNG)
        );
        assert_eq!(read(fc.as_bytes()), Some((App::FilmCraft, PNG.to_vec())));
        // EffectCraft: the preview comes first, `savedBy` after it (past the head when large).
        let big = vec![7u8; 20_000];
        let ec = format!(
            "{{\n  \"preview\": {{\"mime\": \"image/png\", \"data\": \"{}\"}},\n  \"savedBy\": \"0.4.0\",\n  \"schema\": 3\n}}",
            b64(&big)
        );
        assert_eq!(read(ec.as_bytes()), Some((App::EffectCraft, big)));
    }

    #[test]
    fn no_preview_or_other_files() {
        let none =
            r#"{"format":"vectorcraft","version":4,"document":{"layers":[{"preview":true}]}}"#;
        assert_eq!(read(none.as_bytes()), None);
        assert_eq!(
            read(br#"{"preview":{"data":"AAAA"},"other":1}"#),
            None,
            "not an ArtCraft file"
        );
        for junk in [
            &b""[..],
            b"{",
            &[0x1f, 0x8b, 1, 2, 3],
            b"{\"preview\": {\"data\": \"AAA",
        ] {
            assert_eq!(read(junk), None);
        }
    }
}
