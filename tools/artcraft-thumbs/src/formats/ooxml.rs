//! Office Open XML packages (`.docx`, `.xlsx` and their macro/template flavours).
//!
//! - The package thumbnail (ECMA-376 Part 2 §8.5): the part the root relationship of type
//!   `.../metadata/thumbnail` names. WordCraft writes the first page there; Office writes one
//!   when asked to.
//! - Without one, a workbook shows its first worksheet's cells ([`Sheet`]), from the cached
//!   values in the file (formulas are not evaluated).

use super::zip::Zip;
use super::{Source, find};

/// Rows and columns of a sheet kept for the thumbnail.
pub const MAX_ROWS: usize = 40;
pub const MAX_COLS: usize = 16;
/// Characters kept per cell.
const MAX_TEXT: usize = 40;

/// The top-left cells of a worksheet: `cells[row][col]`, empty strings for empty cells.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Sheet {
    pub cells: Vec<Vec<String>>,
}

/// The package thumbnail (PNG, JPEG or BMP bytes).
pub fn thumbnail(src: &mut dyn Source, z: &Zip) -> Option<Vec<u8>> {
    let rels = z.read(src, "_rels/.rels")?;
    let rels = String::from_utf8_lossy(&rels);
    let target = elements(&rels, "Relationship")
        .find(|r| attr(r, "Type").is_some_and(|t| t.ends_with("/metadata/thumbnail")))
        .and_then(|r| attr(r, "Target"))?;
    let img = z.read(src, target.trim_start_matches('/'))?;
    // Office may store a metafile (EMF/WMF): not an image this can show.
    let is_image =
        img.starts_with(b"\x89PNG") || img.starts_with(&[0xff, 0xd8]) || img.starts_with(b"BM");
    is_image.then_some(img)
}

/// The first worksheet's top-left cells; `None` when the workbook can't be read.
pub fn sheet(src: &mut dyn Source, z: &Zip) -> Option<Sheet> {
    let path = first_sheet_path(src, z).unwrap_or_else(|| "xl/worksheets/sheet1.xml".into());
    let xml = z.read(src, &path)?;
    let xml = String::from_utf8_lossy(&xml);
    let shared = z
        .read(src, "xl/sharedStrings.xml")
        .map(|b| shared_strings(&String::from_utf8_lossy(&b)))
        .unwrap_or_default();
    let mut cells = vec![vec![String::new(); MAX_COLS]; MAX_ROWS];
    let mut used = false;
    for (start, c) in elements_with_body(&xml, "c") {
        let Some((col, row)) = attr(start, "r").and_then(cell_ref) else {
            continue;
        };
        let Some(slot) = cells.get_mut(row).and_then(|r| r.get_mut(col)) else {
            continue;
        };
        let v = tag_text(c, "v");
        let text = match attr(start, "t") {
            Some("s") => v
                .and_then(|i| i.trim().parse::<usize>().ok())
                .and_then(|i| shared.get(i).cloned()),
            Some("inlineStr") => Some(texts(c)),
            Some("b") => v.map(|b| {
                if b.trim() == "1" {
                    "TRUE".into()
                } else {
                    "FALSE".into()
                }
            }),
            Some("str") | Some("e") => v.map(unescape),
            _ => v.map(number),
        };
        if let Some(t) = text.filter(|t| !t.is_empty()) {
            *slot = t.chars().take(MAX_TEXT).collect();
            used = true;
        }
    }
    used.then_some(Sheet { cells })
}

/// The first sheet's part, from `xl/workbook.xml` and its relationships.
fn first_sheet_path(src: &mut dyn Source, z: &Zip) -> Option<String> {
    let wb = z.read(src, "xl/workbook.xml")?;
    let wb = String::from_utf8_lossy(&wb);
    let id = elements(&wb, "sheet")
        .next()
        .and_then(|s| attr(s, "r:id"))?
        .to_string();
    let rels = z.read(src, "xl/_rels/workbook.xml.rels")?;
    let rels = String::from_utf8_lossy(&rels);
    let target = elements(&rels, "Relationship")
        .find(|r| attr(r, "Id") == Some(id.as_str()))
        .and_then(|r| attr(r, "Target"))?;
    Some(match target.strip_prefix('/') {
        Some(abs) => abs.to_string(),
        None => format!("xl/{target}"),
    })
}

/// `"B3"` → (1, 2).
fn cell_ref(r: &str) -> Option<(usize, usize)> {
    let letters = r.bytes().take_while(u8::is_ascii_alphabetic).count();
    if letters == 0 || letters > 3 {
        return None;
    }
    let col = r.get(..letters)?.bytes().fold(0usize, |n, b| {
        n * 26 + usize::from(b.to_ascii_uppercase() - b'A' + 1)
    });
    let row: usize = r.get(letters..)?.parse().ok()?;
    Some((col.checked_sub(1)?, row.checked_sub(1)?))
}

/// A cached number as a sheet shows it by default: at most 10 significant digits.
fn number(v: &str) -> String {
    let v = v.trim();
    match v.parse::<f64>() {
        Ok(n) if n.is_finite() && n.fract() == 0.0 && n.abs() < 1e15 => format!("{n:.0}"),
        Ok(n) if n.is_finite() => {
            let s = format!("{n:.10}");
            let s = s.trim_end_matches('0').trim_end_matches('.');
            s.chars().take(12).collect()
        }
        _ => unescape(v),
    }
}

fn shared_strings(xml: &str) -> Vec<String> {
    elements_with_body(xml, "si")
        .map(|(_, body)| texts(body))
        .collect()
}

/// The text of every `<t>` in `xml`, joined (rich text runs).
fn texts(xml: &str) -> String {
    let mut out = String::new();
    for (_, body) in elements_with_body(xml, "t") {
        out.push_str(&unescape(body));
    }
    out
}

fn tag_text<'a>(xml: &'a str, tag: &'a str) -> Option<&'a str> {
    elements_with_body(xml, tag).next().map(|(_, b)| b)
}

fn unescape(s: &str) -> String {
    s.replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&quot;", "\"")
        .replace("&apos;", "'")
        .replace("&amp;", "&")
}

/// Start tags `<tag …>` / `<tag …/>` (the text inside `<` and `>`).
fn elements<'a>(xml: &'a str, tag: &'a str) -> impl Iterator<Item = &'a str> + 'a {
    elements_with_body(xml, tag).map(|(start, _)| start)
}

/// Each `<tag …>body</tag>` (or empty `<tag …/>`): the start tag's text and the body. Matches the
/// local name with or without a namespace prefix; nested same-name elements are not expected.
fn elements_with_body<'a>(
    xml: &'a str,
    tag: &'a str,
) -> impl Iterator<Item = (&'a str, &'a str)> + 'a {
    let mut pos = 0;
    std::iter::from_fn(move || {
        loop {
            let rest = xml.get(pos..)?;
            let lt = rest.find('<')?;
            let after = rest.get(lt + 1..)?;
            let name_len = after
                .find(|c: char| c.is_whitespace() || c == '>' || c == '/')
                .unwrap_or(after.len());
            let name = after.get(..name_len)?;
            let local = name.rsplit(':').next().unwrap_or(name);
            let gt = after.find('>')?;
            pos += lt + 1 + gt + 1;
            if local != tag
                || name.starts_with('/')
                || name.starts_with('?')
                || name.starts_with('!')
            {
                continue;
            }
            let start = after.get(..gt)?;
            if start.ends_with('/') {
                return Some((start.trim_end_matches('/'), ""));
            }
            let body_rest = xml.get(pos..)?;
            let close = format!("</{name}>");
            let end = body_rest.find(&close)?;
            let body = body_rest.get(..end)?;
            pos += end + close.len();
            return Some((start, body));
        }
    })
}

/// An attribute's value in a start tag's text.
fn attr<'a>(start: &'a str, name: &str) -> Option<&'a str> {
    let mut rest = start;
    loop {
        let at = find(rest.as_bytes(), name.as_bytes())?;
        let before_ok = at == 0
            || rest
                .as_bytes()
                .get(at - 1)
                .is_some_and(|b| b.is_ascii_whitespace());
        let after = rest.get(at + name.len()..)?.trim_start();
        if before_ok && let Some(v) = after.strip_prefix('=') {
            let v = v.trim_start();
            let q = v.chars().next()?;
            if q == '"' || q == '\'' {
                let v = v.get(1..)?;
                return v.get(..v.find(q)?);
            }
        }
        rest = rest.get(at + name.len()..)?;
    }
}

#[cfg(test)]
mod tests {
    use super::super::zip::build;
    use super::*;

    const RELS: &str = r#"<?xml version="1.0"?><Relationships xmlns="x"><Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/officeDocument" Target="word/document.xml"/><Relationship Id="rId9" Type="http://schemas.openxmlformats.org/package/2006/relationships/metadata/thumbnail" Target="docProps/thumbnail.jpeg"/></Relationships>"#;

    #[test]
    fn the_thumbnail_comes_from_the_root_relationship() {
        let z = build(
            &[
                ("_rels/.rels", RELS.as_bytes()),
                ("word/document.xml", b"<w/>"),
                ("docProps/thumbnail.jpeg", &[0xff, 0xd8, 1]),
            ],
            true,
        );
        let mut src = z.as_slice();
        let zip = Zip::open(&mut src).unwrap();
        assert_eq!(thumbnail(&mut src, &zip), Some(vec![0xff, 0xd8, 1]));
        // a metafile is not shown
        let z = build(
            &[
                ("_rels/.rels", RELS.as_bytes()),
                ("docProps/thumbnail.jpeg", b"\x01\x00\x00\x00emf"),
            ],
            false,
        );
        let mut src = z.as_slice();
        let zip = Zip::open(&mut src).unwrap();
        assert_eq!(thumbnail(&mut src, &zip), None);
    }

    #[test]
    fn the_first_sheet_shows_its_cells() {
        let wb = r#"<workbook xmlns:r="r"><sheets><sheet name="Budget" sheetId="1" r:id="rId3"/><sheet name="B" sheetId="2" r:id="rId1"/></sheets></workbook>"#;
        let wbrels = r#"<Relationships><Relationship Id="rId1" Type="ws" Target="worksheets/sheet2.xml"/><Relationship Id="rId3" Type="ws" Target="/xl/worksheets/sheet1.xml"/></Relationships>"#;
        let ss = r#"<sst><si><t>Item</t></si><si><r><t>Rent &amp; </t></r><r><t>bills</t></r></si></sst>"#;
        let ws = r#"<worksheet><sheetData><row r="1"><c r="A1" t="s"><v>0</v></c><c r="B1" t="inlineStr"><is><t>Cost</t></is></c></row><row r="2"><c r="A2" t="s"><v>1</v></c><c r="B2"><f>SUM(1,2)</f><v>1200.5</v></c><c r="C2" t="b"><v>1</v></c><c r="Z99"><v>9</v></c></row></sheetData></worksheet>"#;
        let z = build(
            &[
                ("xl/workbook.xml", wb.as_bytes()),
                ("xl/_rels/workbook.xml.rels", wbrels.as_bytes()),
                ("xl/sharedStrings.xml", ss.as_bytes()),
                ("xl/worksheets/sheet1.xml", ws.as_bytes()),
                ("xl/worksheets/sheet2.xml", b"<worksheet/>"),
            ],
            true,
        );
        let mut src = z.as_slice();
        let zip = Zip::open(&mut src).unwrap();
        let s = sheet(&mut src, &zip).unwrap();
        assert_eq!(&s.cells[0][..3], ["Item", "Cost", ""]);
        assert_eq!(&s.cells[1][..3], ["Rent & bills", "1200.5", "TRUE"]);
    }

    #[test]
    fn cell_refs_and_numbers() {
        assert_eq!(cell_ref("A1"), Some((0, 0)));
        assert_eq!(cell_ref("AB12"), Some((27, 11)));
        assert_eq!(cell_ref("A0"), None);
        assert_eq!(cell_ref("12"), None);
        assert_eq!(number("3"), "3");
        assert_eq!(number("0.30000000000000004"), "0.3");
        assert_eq!(number("abc"), "abc");
    }
}
