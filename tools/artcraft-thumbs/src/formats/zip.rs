//! Just enough ZIP to pull single entries out of a package (STORE or DEFLATE; no ZIP64,
//! encryption or multi-disk archives): the central directory is read once, then each entry
//! straight from its offset.

use std::collections::HashMap;
use std::io::Read;

use super::{Source, read_vec};

/// Largest entry read.
pub const MAX_ENTRY: usize = 64 << 20;
/// Largest central directory read (65535 entries of ordinary names fit easily).
const MAX_CENTRAL: usize = 32 << 20;

struct Entry {
    method: u16,
    compressed: usize,
    uncompressed: usize,
    local: u64,
}

pub struct Zip {
    entries: HashMap<String, Entry>,
}

fn u16_at(b: &[u8], at: usize) -> Option<u16> {
    Some(u16::from_le_bytes(b.get(at..at + 2)?.try_into().ok()?))
}

fn u32_at(b: &[u8], at: usize) -> Option<u32> {
    Some(u32::from_le_bytes(b.get(at..at + 4)?.try_into().ok()?))
}

impl Zip {
    /// The archive's directory, `None` when it isn't a ZIP this can read.
    pub fn open(src: &mut dyn Source) -> Option<Zip> {
        let len = src.size()?;
        // End of central directory: in the last 22 + 65535 (comment) bytes.
        let tail_len = len.min(22 + 0xffff);
        let tail = read_vec(src, len - tail_len, usize::try_from(tail_len).ok()?)?;
        let eocd = (0..tail.len().checked_sub(21)?)
            .rev()
            .find(|&i| u32_at(&tail, i) == Some(0x0605_4b50))?;
        let cd_size = u32_at(&tail, eocd + 12)? as usize;
        let cd_offset = u64::from(u32_at(&tail, eocd + 16)?);
        if cd_size > MAX_CENTRAL {
            return None;
        }
        let cd = read_vec(src, cd_offset, cd_size)?;
        let mut entries = HashMap::new();
        let mut p = 0;
        while u32_at(&cd, p) == Some(0x0201_4b50) {
            let n = u16_at(&cd, p + 28)? as usize;
            let skip = u16_at(&cd, p + 30)? as usize + u16_at(&cd, p + 32)? as usize;
            let name = String::from_utf8_lossy(cd.get(p + 46..p + 46 + n)?).into_owned();
            let e = Entry {
                method: u16_at(&cd, p + 10)?,
                compressed: u32_at(&cd, p + 20)? as usize,
                uncompressed: u32_at(&cd, p + 24)? as usize,
                local: u64::from(u32_at(&cd, p + 42)?),
            };
            entries.insert(name, e);
            p += 46 + n + skip;
        }
        Some(Zip { entries })
    }

    pub fn has(&self, name: &str) -> bool {
        self.entries.contains_key(name)
    }

    /// An entry's bytes (at most [`MAX_ENTRY`]).
    pub fn read(&self, src: &mut dyn Source, name: &str) -> Option<Vec<u8>> {
        let e = self.entries.get(name)?;
        if e.compressed > MAX_ENTRY || e.uncompressed > MAX_ENTRY {
            return None;
        }
        let header = read_vec(src, e.local, 30)?;
        if u32_at(&header, 0) != Some(0x0403_4b50) {
            return None;
        }
        let data_at =
            e.local + 30 + u64::from(u16_at(&header, 26)?) + u64::from(u16_at(&header, 28)?);
        let data = read_vec(src, data_at, e.compressed)?;
        match e.method {
            0 => Some(data),
            8 => {
                let mut out = Vec::with_capacity(e.uncompressed);
                flate2::read::DeflateDecoder::new(data.as_slice())
                    .take(MAX_ENTRY as u64)
                    .read_to_end(&mut out)
                    .ok()?;
                Some(out)
            }
            _ => None,
        }
    }
}

/// A STORE or DEFLATE zip, for tests.
#[cfg(test)]
pub(crate) fn build(entries: &[(&str, &[u8])], deflate: bool) -> Vec<u8> {
    use std::io::Write;
    let (mut out, mut cd) = (Vec::new(), Vec::new());
    for (name, data) in entries {
        let body = if deflate {
            let mut e =
                flate2::write::DeflateEncoder::new(Vec::new(), flate2::Compression::default());
            e.write_all(data).unwrap();
            e.finish().unwrap()
        } else {
            data.to_vec()
        };
        let method: u16 = if deflate { 8 } else { 0 };
        let off = out.len() as u32;
        out.extend_from_slice(&0x0403_4b50u32.to_le_bytes());
        out.extend_from_slice(&[20, 0, 0, 8]);
        out.extend_from_slice(&method.to_le_bytes());
        out.extend_from_slice(&[0, 0, 0x21, 0]);
        out.extend_from_slice(&0u32.to_le_bytes());
        out.extend_from_slice(&(body.len() as u32).to_le_bytes());
        out.extend_from_slice(&(data.len() as u32).to_le_bytes());
        out.extend_from_slice(&(name.len() as u16).to_le_bytes());
        out.extend_from_slice(&3u16.to_le_bytes());
        out.extend_from_slice(name.as_bytes());
        out.extend_from_slice(b"xyz");
        out.extend_from_slice(&body);
        cd.extend_from_slice(&0x0201_4b50u32.to_le_bytes());
        cd.extend_from_slice(&[20, 0, 20, 0, 0, 8]);
        cd.extend_from_slice(&method.to_le_bytes());
        cd.extend_from_slice(&[0, 0, 0x21, 0]);
        cd.extend_from_slice(&0u32.to_le_bytes());
        cd.extend_from_slice(&(body.len() as u32).to_le_bytes());
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_store_and_deflate_entries() {
        for deflate in [false, true] {
            let z = build(
                &[("a.txt", b"hello hello hello"), ("dir/b.bin", &[1, 2, 3])],
                deflate,
            );
            let mut src = z.as_slice();
            let zip = Zip::open(&mut src).unwrap();
            assert!(zip.has("dir/b.bin") && !zip.has("c"));
            assert_eq!(zip.read(&mut src, "a.txt").unwrap(), b"hello hello hello");
            assert_eq!(zip.read(&mut src, "dir/b.bin").unwrap(), [1, 2, 3]);
        }
    }

    #[test]
    fn junk_is_not_a_zip() {
        for junk in [&b""[..], b"PK", b"PK\x03\x04garbage", b"PK\x05\x06"] {
            let mut src = junk;
            assert!(Zip::open(&mut src).is_none());
        }
    }
}
