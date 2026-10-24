//! The package thumbnail: a picture of the first page that file browsers show
//! (`docProps/thumbnail.png`, OPC relationship `metadata/thumbnail`, ECMA-376 Part 2 §8.5).

use crate::{DocxError, Flavor};
use wordcraft_doc::Document;

const PART: &str = "docProps/thumbnail.png";
const REL_TYPE: &str = "http://schemas.openxmlformats.org/package/2006/relationships/metadata/thumbnail";

/// [`crate::write_as`], with `png` stored as the package thumbnail.
pub fn write_as_with_thumbnail(doc: &Document, flavor: Flavor, png: &[u8]) -> Result<Vec<u8>, DocxError> {
    let mut parts = crate::write::write_parts(doc, flavor)?;
    add(&mut parts, png);
    crate::package::zip_entries(&parts)
}

/// Add the thumbnail part to written parts: its root relationship and, unless PNG already has
/// one, a content type default. Parts that aren't there (none written) are left alone.
fn add(parts: &mut Vec<(String, Vec<u8>)>, png: &[u8]) {
    let mut done = false;
    for (name, bytes) in parts.iter_mut() {
        match name.as_str() {
            "_rels/.rels" => {
                let rel = format!(r#"<Relationship Id="rIdThumbnail" Type="{REL_TYPE}" Target="{PART}"/>"#);
                done = insert_before(bytes, b"</Relationships>", rel.as_bytes());
            }
            "[Content_Types].xml" if !contains(bytes, br#"Extension="png""#) => {
                insert_before(bytes, b"</Types>", br#"<Default Extension="png" ContentType="image/png"/>"#);
            }
            _ => {}
        }
    }
    if done {
        parts.push((PART.into(), png.to_vec()));
    }
}

fn contains(hay: &[u8], needle: &[u8]) -> bool {
    hay.windows(needle.len()).any(|w| w == needle)
}

/// Insert `what` before the last `before`; false when there is none.
fn insert_before(bytes: &mut Vec<u8>, before: &[u8], what: &[u8]) -> bool {
    match bytes.windows(before.len()).rposition(|w| w == before) {
        Some(at) => {
            bytes.splice(at..at, what.iter().copied());
            true
        }
        None => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_thumbnail_is_a_related_part_with_a_content_type() {
        let mut doc = Document::new();
        doc.ensure_nonempty();
        let png = b"\x89PNG\r\n\x1a\nfake";
        let bytes = write_as_with_thumbnail(&doc, Flavor::Document, png).unwrap();
        let parts = crate::write::write_parts(&doc, Flavor::Document).unwrap();
        let mut with = parts.clone();
        add(&mut with, png);
        let get = |name: &str| with.iter().find(|(n, _)| n == name).map(|(_, b)| String::from_utf8_lossy(b).into_owned()).unwrap();
        assert!(
            get("_rels/.rels").contains(
                r#"Type="http://schemas.openxmlformats.org/package/2006/relationships/metadata/thumbnail" Target="docProps/thumbnail.png""#
            )
        );
        assert_eq!(get("[Content_Types].xml").matches(r#"Extension="png""#).count(), 1);
        assert_eq!(with.iter().find(|(n, _)| n == PART).unwrap().1, png.to_vec());
        // the package still opens as the same document
        assert_eq!(crate::read(&bytes).unwrap(), crate::read(&crate::write(&doc).unwrap()).unwrap());
    }
}
