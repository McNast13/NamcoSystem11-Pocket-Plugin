//! Just enough of the zip format for MAME romsets: the central directory (names and CRCs, read without unpacking
//! anything) and stored or deflated members, CRC-checked on reading as Python's zipfile does. A zip can be a file
//! on the card or held in memory (a zip found inside another zip).

use std::fs::File;
use std::io::{Read, Seek, SeekFrom};
use std::path::PathBuf;
use std::rc::Rc;

pub struct Entry {
    pub name: String,
    pub crc: u32,
    method: u16,
    flags: u16,
    compressed: u64,
    size: u64,
    local: u64,
}

enum Source {
    File(PathBuf),
    Memory(Rc<Vec<u8>>),
}

pub struct Zip {
    /// How the zip is shown to the user: its path below the folder, and "outer.zip > inner.zip" for a nested one.
    pub label: String,
    source: Source,
    pub entries: Vec<Entry>,
}

fn u16_at(b: &[u8], at: usize) -> u16 { u16::from_le_bytes([b[at], b[at + 1]]) }
fn u32_at(b: &[u8], at: usize) -> u32 { u32::from_le_bytes([b[at], b[at + 1], b[at + 2], b[at + 3]]) }

impl Source {
    fn len(&self) -> std::io::Result<u64> {
        match self {
            Source::File(p) => Ok(std::fs::metadata(p)?.len()),
            Source::Memory(b) => Ok(b.len() as u64),
        }
    }

    fn read_at(&self, at: u64, len: usize) -> std::io::Result<Vec<u8>> {
        match self {
            Source::File(p) => {
                let mut file = File::open(p)?;
                let mut buf = vec![0; len];
                file.seek(SeekFrom::Start(at))?;
                file.read_exact(&mut buf)?;
                Ok(buf)
            }
            Source::Memory(b) => b
                .get(at as usize..at as usize + len)
                .map(|s| s.to_vec())
                .ok_or_else(|| std::io::Error::from(std::io::ErrorKind::UnexpectedEof)),
        }
    }
}

impl Zip {
    pub fn open(path: PathBuf, label: String) -> Result<Zip, String> { Zip::load(Source::File(path), label) }

    pub fn from_bytes(data: Vec<u8>, label: String) -> Result<Zip, String> { Zip::load(Source::Memory(Rc::new(data)), label) }

    fn load(source: Source, label: String) -> Result<Zip, String> {
        let bad = |why: &str| format!("{label}: {why}");
        let len = source.len().map_err(|e| bad(&e.to_string()))?;
        // The end-of-central-directory record is the last 22 bytes, before a comment of up to 64 KB.
        let tail_len = len.min(22 + 0xFFFF) as usize;
        let tail = source.read_at(len - tail_len as u64, tail_len).map_err(|e| bad(&e.to_string()))?;
        let eocd = (0..tail_len.saturating_sub(21)).rev().find(|&i| u32_at(&tail, i) == 0x0605_4b50)
            .ok_or_else(|| bad("not a zip file"))?;
        let count = u16_at(&tail, eocd + 10) as usize;
        let cd_size = u32_at(&tail, eocd + 12);
        let cd_offset = u32_at(&tail, eocd + 16);
        if count == 0xFFFF || cd_size == 0xFFFF_FFFF || cd_offset == 0xFFFF_FFFF {
            return Err(bad("zip64 archives are not supported (re-zip it as a normal zip)"));
        }
        let cd = source.read_at(cd_offset as u64, cd_size as usize).map_err(|_| bad("damaged zip (central directory)"))?;
        let mut entries = Vec::with_capacity(count);
        let mut at = 0;
        for _ in 0..count {
            if at + 46 > cd.len() || u32_at(&cd, at) != 0x0201_4b50 {
                return Err(bad("damaged zip (central directory)"));
            }
            let (name_len, extra_len, comment_len) =
                (u16_at(&cd, at + 28) as usize, u16_at(&cd, at + 30) as usize, u16_at(&cd, at + 32) as usize);
            let name = cd.get(at + 46..at + 46 + name_len).ok_or_else(|| bad("damaged zip (central directory)"))?;
            entries.push(Entry {
                name: String::from_utf8_lossy(name).into_owned(),
                flags: u16_at(&cd, at + 8),
                method: u16_at(&cd, at + 10),
                crc: u32_at(&cd, at + 16),
                compressed: u32_at(&cd, at + 20) as u64,
                size: u32_at(&cd, at + 24) as u64,
                local: u32_at(&cd, at + 42) as u64,
            });
            at += 46 + name_len + extra_len + comment_len;
        }
        Ok(Zip { label, source, entries })
    }

    pub fn read(&self, entry: &Entry) -> Result<Vec<u8>, String> {
        let bad = |why: &str| format!("{} > {}: {why}", self.label, entry.name);
        if entry.flags & 1 != 0 {
            return Err(bad("encrypted"));
        }
        let local = self.source.read_at(entry.local, 30).map_err(|_| bad("damaged zip"))?;
        if u32_at(&local, 0) != 0x0403_4b50 {
            return Err(bad("damaged zip"));
        }
        let start = entry.local + 30 + u16_at(&local, 26) as u64 + u16_at(&local, 28) as u64;
        let raw = self.source.read_at(start, entry.compressed as usize).map_err(|_| bad("damaged zip (truncated)"))?;
        let data = match entry.method {
            0 => raw,
            8 => miniz_oxide::inflate::decompress_to_vec_with_limit(&raw, entry.size as usize)
                .map_err(|_| bad("damaged zip (bad deflate data)"))?,
            m => return Err(bad(&format!("compression method {m} is not supported (re-zip it with normal zip compression)"))),
        };
        if data.len() as u64 != entry.size || crc32fast::hash(&data) != entry.crc {
            return Err(bad("damaged zip (CRC check failed when unpacking)"));
        }
        Ok(data)
    }
}
