//! TIFF metadata attachment without re-encoding pixels. Classic TIFF (II/MM, version 42) only.
//! Appends a replacement first IFD; existing image-data offsets, other IFDs, and pixel bytes stay put.
//! Unicode document metadata is losslessly preserved in tag 700 (XMP). Baseline ASCII tags use
//! ASCII fallback; EXIF UserComment uses ASCII only when representable (otherwise XMP carries it).
use dtool_core::metadata::DocMetadata;
use std::fmt;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Error { InvalidTiff, UnsupportedBigTiff, TooLarge }
impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result { write!(f, "TIFF metadata: {self:?}") }
}
impl std::error::Error for Error {}

#[derive(Clone, Copy)]
struct Order(bool);
impl Order {
    fn u16(self, b: &[u8]) -> u16 { if self.0 { u16::from_le_bytes([b[0], b[1]]) } else { u16::from_be_bytes([b[0], b[1]]) } }
    fn u32(self, b: &[u8]) -> u32 { if self.0 { u32::from_le_bytes(b[..4].try_into().unwrap()) } else { u32::from_be_bytes(b[..4].try_into().unwrap()) } }
    fn b16(self, n: u16) -> [u8; 2] { if self.0 { n.to_le_bytes() } else { n.to_be_bytes() } }
    fn b32(self, n: u32) -> [u8; 4] { if self.0 { n.to_le_bytes() } else { n.to_be_bytes() } }
}
fn offset(n: usize) -> Result<u32, Error> { u32::try_from(n).map_err(|_| Error::TooLarge) }
fn align(bytes: &mut Vec<u8>) { if bytes.len() % 2 != 0 { bytes.push(0); } }

#[derive(Debug, Clone)]
struct Tag { id: u16, kind: u16, data: Vec<u8>, count: u32 }
fn ascii(text: &str) -> Vec<u8> {
    text.chars().map(|c| if c.is_ascii() && c != '\0' { c as u8 } else { b'?' }).chain([0]).collect()
}
fn ascii_tag(id: u16, text: &str) -> Result<Tag, Error> {
    let data = ascii(text);
    Ok(Tag { id, kind: 2, count: offset(data.len())?, data })
}
fn tag(id: u16, kind: u16, data: Vec<u8>) -> Result<Tag, Error> {
    Ok(Tag { id, kind, count: offset(data.len())?, data })
}

/// Build a raw 12-byte TIFF entry, appending payload if it does not fit inline.
fn entry(out: &mut Vec<u8>, order: Order, tag: Tag) -> Result<[u8; 12], Error> {
    let mut result = [0; 12];
    result[..2].copy_from_slice(&order.b16(tag.id));
    result[2..4].copy_from_slice(&order.b16(tag.kind));
    result[4..8].copy_from_slice(&order.b32(tag.count));
    if tag.data.len() <= 4 {
        result[8..8 + tag.data.len()].copy_from_slice(&tag.data);
    } else {
        align(out);
        let pos = offset(out.len())?;
        offset(out.len().checked_add(tag.data.len()).ok_or(Error::TooLarge)?)?;
        out.extend(tag.data);
        result[8..12].copy_from_slice(&order.b32(pos));
    }
    Ok(result)
}

fn directory(out: &mut Vec<u8>, order: Order, mut entries: Vec<[u8; 12]>, next: u32) -> Result<u32, Error> {
    let count = u16::try_from(entries.len()).map_err(|_| Error::TooLarge)?;
    align(out);
    let pos = offset(out.len())?;
    let size = 2usize + entries.len() * 12 + 4;
    offset(out.len().checked_add(size).ok_or(Error::TooLarge)?)?;
    entries.sort_by_key(|e| order.u16(e));
    out.extend(order.b16(count));
    for e in entries { out.extend(e); }
    out.extend(order.b32(next));
    Ok(pos)
}

fn metadata_tags(meta: &DocMetadata) -> Result<Vec<Tag>, Error> {
    let mut tags = Vec::new();
    for (id, text) in [(269, &meta.title), (270, &meta.description), (315, &meta.author), (33432, &meta.copyright)] {
        if !text.is_empty() { tags.push(ascii_tag(id, text)?); }
    }
    tags.push(tag(700, 1, dtool_core::metadata::write_xmp(meta).into_bytes())?);
    Ok(tags)
}

/// Add/update ICC (34675), XMP (700), baseline metadata, and EXIF tags in a classic TIFF.
/// None leaves that category untouched. Empty ICC is invalid. Old metadata tags are replaced only
/// when the new metadata supplies them; existing unrelated tags and subsequent page IFDs survive.
/// EXIF is created only when absent (existing camera EXIF is kept intact, XMP gets new description).
/// Output is a fresh buffer; errors do not modify the input. No flags/options are silently dropped.
pub fn attach(bytes: &[u8], icc: Option<&[u8]>, metadata: Option<&DocMetadata>) -> Result<Vec<u8>, Error> {
    if bytes.len() < 8 { return Err(Error::InvalidTiff); }
    let order = match &bytes[..2] { b"II" => Order(true), b"MM" => Order(false), _ => return Err(Error::InvalidTiff) };
    match order.u16(&bytes[2..4]) { 42 => (), 43 => return Err(Error::UnsupportedBigTiff), _ => return Err(Error::InvalidTiff) }
    let pos = order.u32(&bytes[4..8]) as usize;
    if pos < 8 || pos.checked_add(2).is_none_or(|n| n > bytes.len()) { return Err(Error::InvalidTiff); }
    let count = order.u16(&bytes[pos..pos + 2]) as usize;
    let end = pos.checked_add(2 + count * 12 + 4).ok_or(Error::InvalidTiff)?;
    if end > bytes.len() { return Err(Error::InvalidTiff); }
    let mut entries: Vec<[u8; 12]> = bytes[pos + 2..end - 4].chunks_exact(12).map(|c| c.try_into().unwrap()).collect();
    // Validate payload references (including offsets of tags we retain). Never reinterpret or move them.
    for e in &entries {
        let unit: usize = match order.u16(&e[2..4]) { 1 | 2 | 6 | 7 => 1, 3 | 8 => 2, 4 | 9 | 11 | 13 => 4, 5 | 10 | 12 => 8, _ => return Err(Error::InvalidTiff) };
        let size = (order.u32(&e[4..8]) as usize).checked_mul(unit).ok_or(Error::InvalidTiff)?;
        if size > 4 {
            let start = order.u32(&e[8..12]) as usize;
            if start < 8 || start.checked_add(size).is_none_or(|n| n > bytes.len()) { return Err(Error::InvalidTiff); }
        }
    }
    if icc.is_some_and(|p| p.is_empty()) { return Err(Error::InvalidTiff); }
    if icc.is_none() && metadata.is_none() { return Ok(bytes.to_vec()); }
    let next = order.u32(&bytes[end - 4..end]);
    let mut out = bytes.to_vec();
    let mut tags = if let Some(meta) = metadata { metadata_tags(meta)? } else { Vec::new() };
    if let Some(profile) = icc { tags.push(tag(34675, 7, profile.to_vec())?); }
    if let Some(meta) = metadata {
        if !entries.iter().any(|e| order.u16(e) == 34665) {
            let version = entry(&mut out, order, tag(36864, 7, b"0232".to_vec())?)?;
            let mut exif = vec![version];
            if !meta.description.is_empty() && meta.description.is_ascii() {
                let mut comment = b"ASCII\0\0\0".to_vec();
                comment.extend_from_slice(meta.description.as_bytes());
                exif.push(entry(&mut out, order, tag(37510, 7, comment)?)?);
            }
            let exif_offset = directory(&mut out, order, exif, 0)?;
            tags.push(Tag { id: 34665, kind: 4, count: 1, data: order.b32(exif_offset).to_vec() });
        }
    }
    for tag in tags {
        entries.retain(|e| order.u16(e) != tag.id);
        entries.push(entry(&mut out, order, tag)?);
    }
    let first = directory(&mut out, order, entries, next)?;
    out[4..8].copy_from_slice(&order.b32(first));
    Ok(out)
}
