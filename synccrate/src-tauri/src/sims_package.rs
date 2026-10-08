//! Sims 4 `.package` files (DBPF 2.1): a thumbnail, and for build/buy objects
//! the catalog name and description, read from the package's own resources.
//!
//! Only the header, the index and the few resources we need are read, so a
//! merged package of several GB costs a few small reads. Checked against real
//! CC (2026-10): about 2 in 3 packages carry a thumbnail, stored as a JFIF
//! with an extra `ALFA` APP0 block (the alpha mask) that plain JPEG decoders
//! skip; resources were zlib-compressed or stored, never RefPack. CAS parts
//! name themselves with internal ids (`Creator_yfBody_Dress_2025...`), so
//! their readable name comes from the file name instead.
//!
//! Packages come from friends via sync, i.e. untrusted: every count, offset
//! and size is bounded before it's used, and decompression is capped.
use std::io::{Read, Seek, SeekFrom};

pub const T_STBL: u32 = 0x220557DA;
pub const T_COBJ: u32 = 0x319E4F1D;
pub const T_CASP: u32 = 0x034AEECB;
/// CAS part, build/buy and body part thumbnails.
pub const THUMB_TYPES: [u32; 3] = [0x3C1AF1F2, 0x3C2A8647, 0x5B282D45];

const COMP_NONE: u16 = 0x0000;
const COMP_ZLIB: u16 = 0x5A42;
/// 500k entries is a 16 MB index; real merged packages stay far below.
const MAX_INDEX_BYTES: u64 = 16 * 1024 * 1024;
const MAX_STBL_BYTES: u32 = 4 * 1024 * 1024;
const MAX_COBJ_BYTES: u32 = 64 * 1024;
/// English string tables have locale 0x00 in the instance's top byte.
const LOCALE_EN: u8 = 0x00;

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Entry {
    pub rtype: u32,
    pub group: u32,
    pub instance: u64,
    pub offset: u32,
    pub size: u32,
    pub mem_size: u32,
    pub compression: u16,
}

impl Entry {
    fn readable(&self) -> bool {
        matches!(self.compression, COMP_NONE | COMP_ZLIB)
    }
}

/// Where a package's thumbnail sits, to read it again when a row shows it.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ResourceRef {
    pub offset: u32,
    pub size: u32,
    pub mem_size: u32,
    pub compression: u16,
}

impl From<&Entry> for ResourceRef {
    fn from(e: &Entry) -> Self {
        Self { offset: e.offset, size: e.size, mem_size: e.mem_size, compression: e.compression }
    }
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct PackageInfo {
    /// Catalog name of the first build/buy object (None for CAS-only packages).
    pub object_name: Option<String>,
    /// Catalog description of that object (raw; may hold `<font>` tags).
    pub object_desc: Option<String>,
    /// Build/buy objects plus CAS parts (each swatch is its own CAS part).
    pub items: u32,
    pub thumbnail: Option<ResourceRef>,
}

fn u16_at(b: &[u8], p: usize) -> Option<u16> {
    Some(u16::from_le_bytes(b.get(p..p + 2)?.try_into().ok()?))
}
fn u32_at(b: &[u8], p: usize) -> Option<u32> {
    Some(u32::from_le_bytes(b.get(p..p + 4)?.try_into().ok()?))
}
fn u64_at(b: &[u8], p: usize) -> Option<u64> {
    Some(u64::from_le_bytes(b.get(p..p + 8)?.try_into().ok()?))
}

/// The resource index of a DBPF 2.1 package, or None for anything else
/// (including Sims 3's DBPF 2.0 and Sims 2's 1.x).
pub fn read_index<R: Read + Seek>(r: &mut R, file_len: u64) -> Option<Vec<Entry>> {
    let mut hdr = [0u8; 96];
    r.seek(SeekFrom::Start(0)).ok()?;
    r.read_exact(&mut hdr).ok()?;
    if &hdr[..4] != b"DBPF" || u32_at(&hdr, 4)? != 2 || u32_at(&hdr, 8)? != 1 {
        return None;
    }
    let count = u32_at(&hdr, 36)? as u64;
    let index_size = u32_at(&hdr, 44)? as u64;
    // The 64-bit position at 0x40 is the real one; 0x28 is a legacy copy.
    let pos = match u64_at(&hdr, 64)? {
        0 => u32_at(&hdr, 40)? as u64,
        p => p,
    };
    if count == 0 || index_size < 4 || index_size > MAX_INDEX_BYTES || pos.checked_add(index_size)? > file_len {
        return None;
    }
    let mut buf = vec![0u8; index_size as usize];
    r.seek(SeekFrom::Start(pos)).ok()?;
    r.read_exact(&mut buf).ok()?;
    parse_index(&buf, count)
}

/// Index layout: a flags word saying which of type / group / instance-hi /
/// instance-lo are shared by all entries (stored once, right after it), then
/// per entry the rest, offset, size (top bit = has compression fields),
/// memory size and, when flagged, compression type + a committed word.
pub fn parse_index(buf: &[u8], count: u64) -> Option<Vec<Entry>> {
    let flags = u32_at(buf, 0)?;
    let mut p = 4;
    let mut shared = [None; 4];
    for (i, s) in shared.iter_mut().enumerate() {
        if flags & (1 << i) != 0 {
            *s = Some(u32_at(buf, p)?);
            p += 4;
        }
    }
    // Each entry is at least 12 bytes; a count the buffer can't hold is a lie.
    if count > (buf.len() as u64) / 12 {
        return None;
    }
    let mut out = Vec::with_capacity(count as usize);
    for _ in 0..count {
        let mut ids = [0u32; 4];
        for (i, id) in ids.iter_mut().enumerate() {
            *id = match shared[i] {
                Some(v) => v,
                None => {
                    p += 4;
                    u32_at(buf, p - 4)?
                }
            };
        }
        let offset = u32_at(buf, p)?;
        let raw_size = u32_at(buf, p + 4)?;
        let mem_size = u32_at(buf, p + 8)?;
        p += 12;
        let compression = if raw_size & 0x8000_0000 != 0 {
            p += 4;
            u16_at(buf, p - 4)?
        } else {
            COMP_NONE
        };
        out.push(Entry {
            rtype: ids[0],
            group: ids[1],
            instance: ((ids[2] as u64) << 32) | ids[3] as u64,
            offset,
            size: raw_size & 0x7FFF_FFFF,
            mem_size,
            compression,
        });
    }
    Some(out)
}

/// A resource's bytes, decompressed, or None when it's bigger than `max`,
/// out of the file, or compressed with something we don't read.
pub fn read_resource<R: Read + Seek>(r: &mut R, res: &ResourceRef, file_len: u64, max: u32) -> Option<Vec<u8>> {
    if res.mem_size > max || res.size > max.saturating_mul(2).max(1024) || (res.offset as u64).checked_add(res.size as u64)? > file_len {
        return None;
    }
    let mut raw = vec![0u8; res.size as usize];
    r.seek(SeekFrom::Start(res.offset as u64)).ok()?;
    r.read_exact(&mut raw).ok()?;
    match res.compression {
        COMP_NONE => Some(raw),
        COMP_ZLIB => {
            let mut out = Vec::with_capacity(res.mem_size as usize);
            flate2::read::ZlibDecoder::new(&raw[..]).take(max as u64 + 1).read_to_end(&mut out).ok()?;
            (out.len() <= max as usize).then_some(out)
        }
        _ => None,
    }
}

/// String table: `STBL`, version u16, compressed u8, entry count u64, two
/// reserved bytes, total string length u32; then key hash u32, flags u8,
/// length u16 and UTF-8 bytes per entry.
pub fn parse_stbl(b: &[u8]) -> Vec<(u32, String)> {
    let mut out = Vec::new();
    if b.get(..4) != Some(b"STBL") {
        return out;
    }
    let Some(count) = u64_at(b, 7) else { return out };
    let mut p = 21;
    for _ in 0..count.min(b.len() as u64 / 7) {
        let (Some(key), Some(len)) = (u32_at(b, p), u16_at(b, p + 5)) else { break };
        p += 7;
        let Some(bytes) = b.get(p..p + len as usize) else { break };
        out.push((key, String::from_utf8_lossy(bytes).into_owned()));
        p += len as usize;
    }
    out
}

/// Name and description string keys of a catalog object (COBJ).
pub fn cobj_name_keys(b: &[u8]) -> Option<(u32, u32)> {
    Some((u32_at(b, 8)?, u32_at(b, 12)?))
}

/// Everything we show for one package, or None when it isn't DBPF 2.1.
pub fn read_package<R: Read + Seek>(r: &mut R, file_len: u64, max_thumb: u32) -> Option<PackageInfo> {
    let index = read_index(r, file_len)?;
    let mut info = PackageInfo {
        items: index.iter().filter(|e| e.rtype == T_COBJ || e.rtype == T_CASP).count().min(u32::MAX as usize) as u32,
        ..Default::default()
    };
    // The largest thumbnail that fits: merged packages hold one per item and
    // size, and the big ones look best in the details panel.
    info.thumbnail = index
        .iter()
        .filter(|e| THUMB_TYPES.contains(&e.rtype) && e.readable() && e.mem_size > 0 && e.mem_size <= max_thumb)
        .max_by_key(|e| e.mem_size)
        .map(ResourceRef::from);

    // Swatches of one object share its name; different names mean a merged
    // package, which the first object's name would mislabel ("Washing Powder"
    // for a whole merged furniture set). A few catalog entries tell them apart.
    let mut keys: Vec<(u32, u32)> = Vec::new();
    for cobj in index.iter().filter(|e| e.rtype == T_COBJ && e.readable()).take(8) {
        if let Some(k) = read_resource(r, &cobj.into(), file_len, MAX_COBJ_BYTES).as_deref().and_then(cobj_name_keys) {
            if !keys.iter().any(|(n, _)| *n == k.0) {
                keys.push(k);
            }
        }
    }
    if let [(name_key, desc_key)] = keys[..] {
        // English first; a package with only another locale still names it.
        let mut tables: Vec<&Entry> = index.iter().filter(|e| e.rtype == T_STBL && e.readable()).collect();
        tables.sort_by_key(|e| (e.instance >> 56) as u8 != LOCALE_EN);
        for t in tables.into_iter().take(16) {
            let Some(bytes) = read_resource(r, &t.into(), file_len, MAX_STBL_BYTES) else { continue };
            let strings = parse_stbl(&bytes);
            let find = |k: u32| strings.iter().find(|(key, s)| *key == k && !s.trim().is_empty()).map(|(_, s)| s.clone());
            if let Some(name) = find(name_key) {
                info.object_name = Some(name);
                info.object_desc = find(desc_key);
                break;
            }
        }
    }
    Some(info)
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use std::io::{Cursor, Write};

    pub(crate) const JPEG: &[u8] = &[0xFF, 0xD8, 0xFF, 0xE0, 0, 16, b'J', b'F', b'I', b'F', 0, 1, 2, 3];

    pub(crate) fn zlib(data: &[u8]) -> Vec<u8> {
        let mut e = flate2::write::ZlibEncoder::new(Vec::new(), flate2::Compression::default());
        e.write_all(data).unwrap();
        e.finish().unwrap()
    }

    pub(crate) fn stbl(strings: &[(u32, &str)]) -> Vec<u8> {
        let mut b = b"STBL".to_vec();
        b.extend(5u16.to_le_bytes());
        b.push(0);
        b.extend((strings.len() as u64).to_le_bytes());
        b.extend([0, 0]);
        b.extend((strings.iter().map(|(_, s)| s.len() + 1).sum::<usize>() as u32).to_le_bytes());
        for (k, s) in strings {
            b.extend(k.to_le_bytes());
            b.push(0);
            b.extend((s.len() as u16).to_le_bytes());
            b.extend(s.as_bytes());
        }
        b
    }

    pub(crate) fn cobj(name_key: u32, desc_key: u32) -> Vec<u8> {
        let mut b = 26u32.to_le_bytes().to_vec();
        b.extend(9u32.to_le_bytes());
        b.extend(name_key.to_le_bytes());
        b.extend(desc_key.to_le_bytes());
        b.extend([0u8; 32]);
        b
    }

    /// A DBPF 2.1 package with these (type, instance, data, zlib?) resources.
    /// The group is shared (flag bit 1), as real packages often do.
    pub(crate) fn package(resources: &[(u32, u64, Vec<u8>, bool)]) -> Vec<u8> {
        let mut body = Vec::new();
        let mut index = 2u32.to_le_bytes().to_vec();
        index.extend(0u32.to_le_bytes()); // shared group
        for (t, inst, data, compress) in resources {
            let stored = if *compress { zlib(data) } else { data.clone() };
            index.extend(t.to_le_bytes());
            index.extend(((inst >> 32) as u32).to_le_bytes());
            index.extend((*inst as u32).to_le_bytes());
            index.extend((96 + body.len() as u32).to_le_bytes());
            index.extend((stored.len() as u32 | 0x8000_0000).to_le_bytes());
            index.extend((data.len() as u32).to_le_bytes());
            index.extend((if *compress { COMP_ZLIB } else { COMP_NONE }).to_le_bytes());
            index.extend(1u16.to_le_bytes());
            body.extend(stored);
        }
        let mut hdr = vec![0u8; 96];
        hdr[..4].copy_from_slice(b"DBPF");
        hdr[4..8].copy_from_slice(&2u32.to_le_bytes());
        hdr[8..12].copy_from_slice(&1u32.to_le_bytes());
        hdr[36..40].copy_from_slice(&(resources.len() as u32).to_le_bytes());
        hdr[44..48].copy_from_slice(&(index.len() as u32).to_le_bytes());
        hdr[64..72].copy_from_slice(&(96 + body.len() as u64).to_le_bytes());
        [hdr, body, index].concat()
    }

    fn read(bytes: &[u8]) -> Option<PackageInfo> {
        read_package(&mut Cursor::new(bytes), bytes.len() as u64, 1024 * 1024)
    }

    #[test]
    fn object_package_gives_name_description_and_largest_thumbnail() {
        let small = [JPEG, &[9; 10]].concat();
        let big = [JPEG, &[7; 400]].concat();
        let pkg = package(&[
            (T_COBJ, 1, cobj(0xAAAA, 0xBBBB), true),
            // A French table first: the English one still wins.
            (T_STBL, 0x0700_0000_0000_0001, stbl(&[(0xAAAA, "Rouleau")]), true),
            (T_STBL, 0x0000_0000_0000_0001, stbl(&[(0xAAAA, "Cinnamon Roll"), (0xBBBB, "by icemunmun")]), true),
            (THUMB_TYPES[1], 1, small, false),
            (THUMB_TYPES[1], 2, big.clone(), true),
        ]);
        let info = read(&pkg).unwrap();
        assert_eq!(info.object_name.as_deref(), Some("Cinnamon Roll"));
        assert_eq!(info.object_desc.as_deref(), Some("by icemunmun"));
        assert_eq!(info.items, 1);
        let t = info.thumbnail.unwrap();
        assert_eq!(t.mem_size as usize, big.len());
        let bytes = read_resource(&mut Cursor::new(&pkg), &t, pkg.len() as u64, 1024 * 1024).unwrap();
        assert_eq!(bytes, big);
    }

    #[test]
    fn cas_package_has_no_object_name_but_counts_swatches() {
        let pkg = package(&[(T_CASP, 1, vec![0; 40], true), (T_CASP, 2, vec![0; 40], true), (THUMB_TYPES[0], 1, JPEG.to_vec(), true)]);
        let info = read(&pkg).unwrap();
        assert_eq!((info.object_name, info.items), (None, 2));
        assert!(info.thumbnail.is_some());
    }

    #[test]
    fn merged_objects_get_no_single_name_but_swatches_do() {
        let strings = stbl(&[(1, "Washing Powder"), (2, "Chic Bathtub")]);
        let merged = package(&[(T_COBJ, 1, cobj(1, 0), true), (T_COBJ, 2, cobj(2, 0), true), (T_STBL, 1, strings.clone(), true)]);
        assert_eq!(read(&merged).unwrap().object_name, None);
        let swatches = package(&[(T_COBJ, 1, cobj(2, 0), true), (T_COBJ, 2, cobj(2, 0), true), (T_STBL, 1, strings, true)]);
        assert_eq!(read(&swatches).unwrap().object_name.as_deref(), Some("Chic Bathtub"));
    }

    #[test]
    fn other_formats_and_lies_are_refused() {
        assert!(read(b"not a package").is_none());
        let mut sims3 = package(&[(T_CASP, 1, vec![0; 8], false)]);
        sims3[8..12].copy_from_slice(&0u32.to_le_bytes()); // DBPF 2.0
        assert!(read(&sims3).is_none());

        // An entry count far beyond what the index can hold.
        let mut pkg = package(&[(T_CASP, 1, vec![0; 8], false)]);
        pkg[36..40].copy_from_slice(&u32::MAX.to_le_bytes());
        assert!(read(&pkg).is_none());

        // An index past the end of the file.
        let mut pkg = package(&[(T_CASP, 1, vec![0; 8], false)]);
        pkg[64..72].copy_from_slice(&(1u64 << 40).to_le_bytes());
        assert!(read(&pkg).is_none());

        // A thumbnail that claims to be bigger than the cap is skipped, and
        // one whose bytes lie past the end isn't read.
        let pkg = package(&[(THUMB_TYPES[0], 1, vec![0xFF; 2048], false)]);
        assert!(read_package(&mut Cursor::new(&pkg), pkg.len() as u64, 1024).unwrap().thumbnail.is_none());
        let bad = ResourceRef { offset: 90, size: 500, mem_size: 500, compression: COMP_NONE };
        assert!(read_resource(&mut Cursor::new(&pkg), &bad, 100, 1024).is_none());
    }

    #[test]
    fn decompression_is_capped() {
        // Claims 10 bytes, inflates to 1 MB (a zip bomb in miniature).
        let data = zlib(&vec![0u8; 1024 * 1024]);
        let res = ResourceRef { offset: 0, size: data.len() as u32, mem_size: 10, compression: COMP_ZLIB };
        assert!(read_resource(&mut Cursor::new(&data), &res, data.len() as u64, 4096).is_none());
        // RefPack and other compressions aren't read.
        let res = ResourceRef { compression: 0xFFFF, ..res };
        assert!(read_resource(&mut Cursor::new(&data), &res, data.len() as u64, 1024 * 1024 * 2).is_none());
    }

    #[test]
    fn stbl_parsing_survives_truncation() {
        let full = stbl(&[(1, "One"), (2, "Two")]);
        assert_eq!(parse_stbl(&full), vec![(1, "One".to_string()), (2, "Two".to_string())]);
        assert_eq!(parse_stbl(&full[..full.len() - 2]), vec![(1, "One".to_string())]);
        assert!(parse_stbl(b"NOPE").is_empty());
    }
}
