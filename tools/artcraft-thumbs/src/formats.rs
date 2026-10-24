//! The preview PNG an ArtCraft document carries, read without the app.
//!
//! - `.pcraft` (PhotoCraft): a ZIP bundle with `thumb.png` (256 px) and, from bigger saves,
//!   `composite/preview.png` (1024 px). See `apps/photocraft/crates/format/src/lib.rs`.
//! - `.vectorcraft` / `.drawcraft` (VectorCraft): JSON, maybe gzip, whose head has
//!   `"preview": {"mime": "image/png", "data": "<base64>"}` right after `format`, `version` and
//!   `generator`. See `apps/vectorcraft/crates/format/src/lib.rs`.
//!
//! Files are untrusted: every size read from them is capped, and nothing here panics.

use std::io::Read;

/// Which app wrote a file (decides the preview reader and the badge).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum App {
    PhotoCraft,
    VectorCraft,
}

impl App {
    /// The app's id (its exe is `<id>.exe`).
    pub fn id(self) -> &'static str {
        match self {
            App::PhotoCraft => "photocraft",
            App::VectorCraft => "vectorcraft",
        }
    }

    /// The app's display name (its default install folder is `%ProgramFiles%\<name>`).
    pub fn name(self) -> &'static str {
        match self {
            App::PhotoCraft => "PhotoCraft",
            App::VectorCraft => "VectorCraft",
        }
    }
}

/// Random access to a file's bytes.
pub trait Source {
    fn size(&mut self) -> Option<u64>;
    /// Exactly `buf.len()` bytes at `offset`, or `None`.
    fn read_at(&mut self, offset: u64, buf: &mut [u8]) -> Option<()>;
}

impl Source for &[u8] {
    fn size(&mut self) -> Option<u64> {
        Some(<[u8]>::len(self) as u64)
    }
    fn read_at(&mut self, offset: u64, buf: &mut [u8]) -> Option<()> {
        let start = usize::try_from(offset).ok()?;
        buf.copy_from_slice(self.get(start..start.checked_add(buf.len())?)?);
        Some(())
    }
}

/// Largest preview PNG read.
const MAX_PNG: usize = 64 << 20;
/// Largest ZIP central directory read (a .pcraft has at most 65535 entries).
const MAX_CENTRAL: usize = 32 << 20;
/// Bytes of a VectorCraft head searched for the preview member.
const VC_HEAD: usize = 4 << 10;
/// Most bytes of a VectorCraft file read for its preview.
const VC_MAX: usize = 48 << 20;

/// The app that wrote the file and its preview PNG, the larger one when `size` (pixels on the
/// longer side) is bigger than the small one.
pub fn preview(src: &mut dyn Source, size: u32) -> Option<(App, Vec<u8>)> {
    let mut magic = [0u8; 4];
    src.read_at(0, &mut magic)?;
    if magic == *b"PK\x03\x04" {
        let names: &[&str] = if size > 256 { &["composite/preview.png", "thumb.png"] } else { &["thumb.png", "composite/preview.png"] };
        return pcraft(src, names).map(|png| (App::PhotoCraft, png));
    }
    vectorcraft(src, magic[..2] == [0x1f, 0x8b]).map(|png| (App::VectorCraft, png))
}

fn u16_at(b: &[u8], at: usize) -> Option<u16> {
    Some(u16::from_le_bytes(b.get(at..at + 2)?.try_into().ok()?))
}

fn u32_at(b: &[u8], at: usize) -> Option<u32> {
    Some(u32::from_le_bytes(b.get(at..at + 4)?.try_into().ok()?))
}

/// The first of `names` a ZIP has (STORE or DEFLATE).
fn pcraft(src: &mut dyn Source, names: &[&str]) -> Option<Vec<u8>> {
    let len = src.size()?;
    // End of central directory: in the last 22 + 65535 (comment) bytes.
    let tail_len = len.min(22 + 0xffff);
    let mut tail = vec![0u8; usize::try_from(tail_len).ok()?];
    src.read_at(len - tail_len, &mut tail)?;
    let eocd = (0..tail.len().checked_sub(21)?).rev().find(|&i| u32_at(&tail, i) == Some(0x0605_4b50))?;
    let cd_size = u32_at(&tail, eocd + 12)? as usize;
    let cd_offset = u64::from(u32_at(&tail, eocd + 16)?);
    if cd_size > MAX_CENTRAL {
        return None;
    }
    let mut cd = vec![0u8; cd_size];
    src.read_at(cd_offset, &mut cd)?;
    let mut found: Vec<Option<(u16, usize, usize, u64)>> = vec![None; names.len()];
    let mut p = 0;
    while u32_at(&cd, p) == Some(0x0201_4b50) {
        let method = u16_at(&cd, p + 10)?;
        let compressed = u32_at(&cd, p + 20)? as usize;
        let uncompressed = u32_at(&cd, p + 24)? as usize;
        let n = u16_at(&cd, p + 28)? as usize;
        let extra = u16_at(&cd, p + 30)? as usize;
        let comment = u16_at(&cd, p + 32)? as usize;
        let local = u64::from(u32_at(&cd, p + 42)?);
        let name = cd.get(p + 46..p + 46 + n)?;
        if let Some(i) = names.iter().position(|w| w.as_bytes() == name) {
            found[i] = Some((method, compressed, uncompressed, local));
        }
        p += 46 + n + extra + comment;
    }
    let (method, compressed, uncompressed, local) = found.into_iter().flatten().next()?;
    if compressed > MAX_PNG || uncompressed > MAX_PNG {
        return None;
    }
    let mut header = [0u8; 30];
    src.read_at(local, &mut header)?;
    if u32_at(&header, 0) != Some(0x0403_4b50) {
        return None;
    }
    let data_at = local + 30 + u64::from(u16_at(&header, 26)?) + u64::from(u16_at(&header, 28)?);
    let mut data = vec![0u8; compressed];
    src.read_at(data_at, &mut data)?;
    match method {
        0 => Some(data),
        8 => {
            let mut out = Vec::with_capacity(uncompressed);
            flate2::read::DeflateDecoder::new(data.as_slice()).take(MAX_PNG as u64).read_to_end(&mut out).ok()?;
            Some(out)
        }
        _ => None,
    }
}

/// Reads a [`Source`] front to back.
struct SourceReader<'a> {
    src: &'a mut dyn Source,
    at: u64,
    len: u64,
}

impl Read for SourceReader<'_> {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        let n = usize::try_from((self.len - self.at).min(buf.len() as u64)).unwrap_or(0);
        let Some(out) = buf.get_mut(..n) else { return Ok(0) };
        if n == 0 {
            return Ok(0);
        }
        self.src.read_at(self.at, out).ok_or_else(|| std::io::Error::other("read failed"))?;
        self.at += n as u64;
        Ok(n)
    }
}

/// The preview of a `.vectorcraft` file (`gz`: gzip-compressed).
fn vectorcraft(src: &mut dyn Source, gz: bool) -> Option<Vec<u8>> {
    let len = src.size()?;
    let reader = SourceReader { src, at: 0, len };
    let mut text: Box<dyn Read> = if gz { Box::new(flate2::read::GzDecoder::new(reader)) } else { Box::new(reader) };
    let mut buf = Vec::new();
    let mut chunk = vec![0u8; 64 << 10];
    // The head first: is this a VectorCraft file with a preview?
    let start = loop {
        let n = text.read(&mut chunk).ok()?;
        buf.extend_from_slice(chunk.get(..n)?);
        if n == 0 || buf.len() >= VC_HEAD {
            let head = buf.get(..VC_HEAD.min(buf.len()))?;
            if head.iter().find(|b| !b.is_ascii_whitespace()) != Some(&b'{') || !(find(head, b"\"vectorcraft\"").is_some() || find(head, b"\"drawcraft\"").is_some()) {
                return None;
            }
            let at = find(head, b"\"preview\"")?;
            // The envelope's member is an object; a layer's `"preview": true` is not it.
            let mut o = at + 9;
            while buf.get(o).is_some_and(|b| b.is_ascii_whitespace() || *b == b':') {
                o += 1;
            }
            if buf.get(o) != Some(&b'{') {
                return None;
            }
            let after = buf.get(o..buf.len().min(o + 256))?;
            let at = o;
            let data = find(after, b"\"data\"")? + at + 6;
            // `: "` then the base64 text.
            let mut i = data;
            while buf.get(i).is_some_and(|b| b.is_ascii_whitespace() || *b == b':') {
                i += 1;
            }
            if buf.get(i) != Some(&b'"') {
                // Not there yet (a huge pretty-printed gap is not a file VectorCraft writes).
                return None;
            }
            break i + 1;
        }
    };
    loop {
        if let Some(end) = buf.get(start..).and_then(|b| b.iter().position(|&c| c == b'"')) {
            return base64_decode(buf.get(start..start + end)?);
        }
        if buf.len() > VC_MAX {
            return None;
        }
        let n = text.read(&mut chunk).ok()?;
        if n == 0 {
            return None;
        }
        buf.extend_from_slice(chunk.get(..n)?);
    }
}

fn find(hay: &[u8], needle: &[u8]) -> Option<usize> {
    hay.windows(needle.len()).position(|w| w == needle)
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
    let clean: Vec<u8> = s.iter().copied().filter(|b| !b.is_ascii_whitespace() && *b != b'\\').collect();
    if !clean.len().is_multiple_of(4) || clean.len() / 4 * 3 > MAX_PNG {
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
mod tests {
    use super::*;

    const PNG: &[u8] = b"\x89PNG fake";

    fn b64(data: &[u8]) -> String {
        const B: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
        let mut s = String::new();
        for c in data.chunks(3) {
            let n = (c[0] as u32) << 16 | (*c.get(1).unwrap_or(&0) as u32) << 8 | *c.get(2).unwrap_or(&0) as u32;
            s.push(B[(n >> 18) as usize & 63] as char);
            s.push(B[(n >> 12) as usize & 63] as char);
            s.push(if c.len() > 1 { B[(n >> 6) as usize & 63] as char } else { '=' });
            s.push(if c.len() > 2 { B[n as usize & 63] as char } else { '=' });
        }
        s
    }

    /// A STORE zip like PhotoCraft's writer makes.
    fn zip(entries: &[(&str, &[u8])]) -> Vec<u8> {
        let (mut out, mut cd) = (Vec::new(), Vec::new());
        for (name, data) in entries {
            let off = out.len() as u32;
            out.extend_from_slice(&0x0403_4b50u32.to_le_bytes());
            out.extend_from_slice(&[20, 0, 0, 8, 0, 0, 0, 0, 0x21, 0]);
            out.extend_from_slice(&0u32.to_le_bytes());
            out.extend_from_slice(&(data.len() as u32).to_le_bytes());
            out.extend_from_slice(&(data.len() as u32).to_le_bytes());
            out.extend_from_slice(&(name.len() as u16).to_le_bytes());
            out.extend_from_slice(&3u16.to_le_bytes());
            out.extend_from_slice(name.as_bytes());
            out.extend_from_slice(b"xyz");
            out.extend_from_slice(data);
            cd.extend_from_slice(&0x0201_4b50u32.to_le_bytes());
            cd.extend_from_slice(&[20, 0, 20, 0, 0, 8, 0, 0, 0, 0, 0x21, 0]);
            cd.extend_from_slice(&0u32.to_le_bytes());
            cd.extend_from_slice(&(data.len() as u32).to_le_bytes());
            cd.extend_from_slice(&(data.len() as u32).to_le_bytes());
            cd.extend_from_slice(&(name.len() as u16).to_le_bytes());
            cd.extend_from_slice(&[0; 12]);
            cd.extend_from_slice(&off.to_le_bytes());
            cd.extend_from_slice(name.as_bytes());
        }
        let cd_off = out.len() as u32;
        out.extend_from_slice(&cd);
        out.extend_from_slice(&0x0605_4b50u32.to_le_bytes());
        out.extend_from_slice(&[0; 4]);
        out.extend_from_slice(&(entries.len() as u16).to_le_bytes());
        out.extend_from_slice(&(entries.len() as u16).to_le_bytes());
        out.extend_from_slice(&(cd.len() as u32).to_le_bytes());
        out.extend_from_slice(&cd_off.to_le_bytes());
        out.extend_from_slice(&0u16.to_le_bytes());
        out
    }

    #[test]
    fn pcraft_thumb_or_composite_by_size() {
        let z = zip(&[("manifest.json", b"{}"), ("thumb.png", b"small"), ("composite/preview.png", b"large")]);
        assert_eq!(preview(&mut z.as_slice(), 96), Some((App::PhotoCraft, b"small".to_vec())));
        assert_eq!(preview(&mut z.as_slice(), 512), Some((App::PhotoCraft, b"large".to_vec())));
        let only_thumb = zip(&[("thumb.png", b"small")]);
        assert_eq!(preview(&mut only_thumb.as_slice(), 1024), Some((App::PhotoCraft, b"small".to_vec())));
        assert_eq!(preview(&mut zip(&[("manifest.json", b"{}")]).as_slice(), 96), None);
    }

    #[test]
    fn vectorcraft_preview_plain_pretty_and_gzip() {
        let compact = format!(r#"{{"format":"vectorcraft","version":4,"generator":"VectorCraft 0.11.0","preview":{{"mime":"image/png","data":"{}"}},"document":{{}}}}"#, b64(PNG));
        assert_eq!(preview(&mut compact.as_bytes(), 256), Some((App::VectorCraft, PNG.to_vec())));
        let pretty = format!("{{\n  \"format\": \"drawcraft\",\n  \"version\": 2,\n  \"preview\": {{\n    \"mime\": \"image/png\",\n    \"data\": \"{}\"\n  }}\n}}", b64(PNG));
        assert_eq!(preview(&mut pretty.as_bytes(), 256), Some((App::VectorCraft, PNG.to_vec())));
        let mut gz = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::default());
        std::io::Write::write_all(&mut gz, compact.as_bytes()).unwrap();
        let gz = gz.finish().unwrap();
        assert_eq!(preview(&mut gz.as_slice(), 256), Some((App::VectorCraft, PNG.to_vec())));
    }

    #[test]
    fn no_preview_or_other_files() {
        let none = r#"{"format":"vectorcraft","version":4,"document":{"layers":[{"preview":true}]}}"#;
        assert_eq!(preview(&mut none.as_bytes(), 256), None);
        assert_eq!(preview(&mut br#"{"preview":{"data":"AAAA"}}"#.as_slice(), 256), None, "not a VectorCraft file");
        for junk in [&b""[..], b"PK", b"PK\x03\x04garbage", &[0x1f, 0x8b, 1, 2, 3]] {
            assert_eq!(preview(&mut &*junk, 256), None);
        }
    }
}
