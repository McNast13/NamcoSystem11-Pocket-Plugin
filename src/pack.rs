//! The game list (src/games.json, made from the core's MRAs by scripts/gen_games.py) and the port of the core's
//! tools/rom_packer.py that turns a set's MAME zips into its ROM images.

use crate::zip::{Entry, Zip};
use serde::Deserialize;
use std::collections::{BTreeMap, HashMap};
use std::path::{Path, PathBuf};

#[derive(Deserialize)]
pub struct Table {
    pub platform: String,
    pub core: String,
    pub bank: usize,
    pub slot_of: Vec<(u32, u32)>,
    pub bios: Bios,
    pub sets: Vec<Set>,
}

#[derive(Deserialize)]
pub struct Bios {
    pub filename: String,
    pub md5: String,
}

#[derive(Deserialize)]
pub struct Set {
    pub set: String,
    pub title: String,
    pub alt: bool,
    pub instance: String,
    pub roms: Vec<Rom>,
}

#[derive(Deserialize)]
pub struct Rom {
    pub index: u32,
    parts: Vec<Part>,
}

#[derive(Deserialize)]
struct Part {
    unit: usize,
    map: u64,
    repeat: u32,
    name: Option<String>,
    #[serde(default)]
    zips: Vec<String>,
    crc: Option<u32>,
    #[serde(default)]
    offset: usize,
    #[serde(default)]
    length: usize,
    data: Option<String>,
}

pub const BIOS_INDEX: u32 = 8;

pub fn table() -> Table { serde_json::from_str(include_str!("games.json")).expect("games.json") }

impl Set {
    /// The zips this set reads its own files from (its zip and its parent's), not the shared BIOS zip.
    pub fn game_zips(&self) -> Vec<&str> {
        let mut out: Vec<&str> = vec![];
        for rom in self.roms.iter().filter(|r| r.index != BIOS_INDEX) {
            for z in rom.parts.iter().flat_map(|p| p.zips.iter()) {
                if !out.contains(&z.as_str()) {
                    out.push(z);
                }
            }
        }
        out
    }

    /// The zips the shared C76 BIOS comes from.
    pub fn bios_zips(&self) -> Vec<&str> {
        self.roms.iter().filter(|r| r.index == BIOS_INDEX).flat_map(|r| r.parts.iter()).flat_map(|p| p.zips.iter())
            .map(|z| z.as_str()).collect()
    }

    pub fn all_zips(&self) -> impl Iterator<Item = &str> {
        self.roms.iter().flat_map(|r| r.parts.iter()).flat_map(|p| p.zips.iter()).map(|z| z.as_str())
    }
}

/// The zips in one folder (the SD card's Assets/<platform>/common) and its subfolders, opened as they are needed.
/// Names are matched ignoring case, as the Pocket's FAT/exFAT card does.
pub struct ZipSet {
    files: HashMap<String, PathBuf>,
    open: HashMap<String, Result<Zip, String>>,
    /// Zips found again in another folder: (the one used, the one ignored).
    pub duplicates: Vec<(PathBuf, PathBuf)>,
}

/// How deep below the folder to look, so a user can copy in a folder of zips (or a folder of folders).
const MAX_DEPTH: usize = 4;

/// A folder's own zips, then its subfolders' in name order; when a zip is in two folders, the first one found is used.
fn walk(dir: &Path, depth: usize, files: &mut HashMap<String, PathBuf>, duplicates: &mut Vec<(PathBuf, PathBuf)>) {
    let Ok(list) = std::fs::read_dir(dir) else { return };
    let mut list: Vec<_> = list.flatten().filter(|f| !f.file_name().to_string_lossy().starts_with('.')).collect();
    list.sort_by_key(|f| f.file_name());
    let mut subdirs = vec![];
    for f in list {
        let (name, path) = (f.file_name().to_string_lossy().to_lowercase(), f.path());
        if path.is_dir() {
            subdirs.push(path);
        } else if name.ends_with(".zip") {
            match files.get(&name) {
                Some(used) => duplicates.push((used.clone(), path)),
                None => {
                    files.insert(name, path);
                }
            }
        }
    }
    if depth < MAX_DEPTH {
        for d in subdirs {
            walk(&d, depth + 1, files, duplicates);
        }
    }
}

enum Found<'a> {
    Entry(&'a Zip, &'a Entry),
    Missing(String),
    Bad(String),
}

impl ZipSet {
    pub fn new(dir: &Path) -> ZipSet {
        let (mut files, mut duplicates) = (HashMap::new(), vec![]);
        walk(dir, 0, &mut files, &mut duplicates);
        ZipSet { files, open: HashMap::new(), duplicates }
    }

    /// The zip files found, by their paths below the folder.
    pub fn names(&self, dir: &Path) -> Vec<String> {
        let mut v: Vec<String> =
            self.files.values().map(|p| p.strip_prefix(dir).unwrap_or(p).display().to_string()).collect();
        v.sort();
        v
    }

    pub fn has(&self, zip: &str) -> bool { self.files.contains_key(&zip.to_lowercase()) }

    /// Forget the opened zips (their directories), so packing one set at a time doesn't keep every zip's list.
    pub fn close(&mut self) { self.open.clear(); }

    fn zip(&mut self, name: &str) -> Option<&Result<Zip, String>> {
        let key = name.to_lowercase();
        let path = self.files.get(&key)?.clone();
        Some(self.open.entry(key).or_insert_with(|| Zip::open(&path)))
    }

    /// Where a part is, looked up as rom_packer.ZipSet.read() does: each zip in turn; a member with the part's name
    /// must have its CRC; otherwise any member with the part's CRC will do (merged sets, renamed files).
    fn find(&mut self, part: &Part) -> Found<'_> {
        let name = part.name.as_deref().unwrap_or_default();
        for z in &part.zips {
            self.zip(z);
        }
        let mut last = None;
        for z in &part.zips {
            let Some(zip) = self.open.get(&z.to_lowercase()) else {
                last.get_or_insert_with(|| format!("{z} is not on the SD card"));
                continue;
            };
            let zip = match zip {
                Ok(zip) => zip,
                Err(e) => return Found::Bad(e.clone()),
            };
            if let Some(e) = zip.find(name) {
                return match part.crc {
                    Some(crc) if e.crc != crc => Found::Bad(format!(
                        "{z}: {name} has CRC {:08x}, the core needs {crc:08x} (a romset from another MAME version?)",
                        e.crc
                    )),
                    _ => Found::Entry(zip, e),
                };
            }
            if let Some(e) = part.crc.and_then(|crc| zip.find_crc(crc)) {
                return Found::Entry(zip, e);
            }
            last = Some(format!("{name} is not in {z}"));
        }
        Found::Missing(last.unwrap_or_else(|| format!("{name}: no zip given")))
    }

    /// Whether every file a set needs is in the zips, with the right CRC (from the zips' directories, nothing unpacked).
    pub fn check(&mut self, set: &Set) -> Result<(), String> {
        let (mut missing, mut zips_looked) = (vec![], vec![]);
        for part in set.roms.iter().flat_map(|r| r.parts.iter()).filter(|p| p.name.is_some()) {
            match self.find(part) {
                Found::Entry(..) => {}
                Found::Bad(e) => return Err(e),
                Found::Missing(_) => {
                    missing.push(part.name.clone().unwrap_or_default());
                    for z in &part.zips {
                        if !zips_looked.contains(z) {
                            zips_looked.push(z.clone());
                        }
                    }
                }
            }
        }
        if missing.is_empty() {
            return Ok(());
        }
        let shown = if missing.len() > 3 { format!("{}, ...", missing[..3].join(", ")) } else { missing.join(", ") };
        let looked: Vec<String> = zips_looked
            .iter()
            .map(|z| if self.has(z) { z.clone() } else { format!("{z} (not on the card)") })
            .collect();
        Err(format!("{} file(s) missing: {shown}; looked in {}", missing.len(), looked.join(", ")))
    }

    /// The set's ROM images by MRA index, as rom_packer.pack() makes them.
    pub fn pack(&mut self, set: &Set) -> Result<BTreeMap<u32, Vec<u8>>, String> {
        let mut out = BTreeMap::new();
        for rom in &set.roms {
            let (mut image, mut cursors) = (vec![], [0usize; 8]);
            for part in &rom.parts {
                let data = match &part.data {
                    Some(hex) => from_hex(hex)?,
                    None => {
                        let data = match self.find(part) {
                            Found::Entry(zip, entry) => zip.read(entry)?,
                            Found::Missing(e) | Found::Bad(e) => return Err(e),
                        };
                        let start = part.offset.min(data.len());
                        let end = if part.length > 0 { (part.offset + part.length).min(data.len()) } else { data.len() };
                        data[start..end.max(start)].to_vec()
                    }
                };
                for _ in 0..part.repeat {
                    place(&mut image, &data, part.unit, part.map, &mut cursors)?;
                }
            }
            out.insert(rom.index, image);
        }
        Ok(out)
    }
}

fn from_hex(hex: &str) -> Result<Vec<u8>, String> {
    (0..hex.len())
        .step_by(2)
        .map(|i| hex.get(i..i + 2).and_then(|b| u8::from_str_radix(b, 16).ok()).ok_or_else(|| format!("bad hex {hex}")))
        .collect()
}

/// rom_packer.place(): write `data` into `image` through an interleave map (the MiSTer loader's rom_data()). The
/// lowest non-zero nibble of the map is the byte lane `idx`; each non-zero nibble v puts the part's next byte at
/// idx + v - 1 (+ the zero nibbles between non-zero ones). cursors[idx] is that lane's next unit, so "01" and "10"
/// parts fill the same words.
pub fn place(image: &mut Vec<u8>, data: &[u8], unit: usize, map: u64, cursors: &mut [usize; 8]) -> Result<(), String> {
    let map = if map == 0 { 1 } else { map };
    let (mut idx, mut m) = (0, map);
    for _ in 0..unit {
        if m & 0xF != 0 {
            break;
        }
        m >>= 4;
        idx += 1;
    }
    if idx >= unit {
        return Err("illegal map".into());
    }
    let (mut offsets, mut first, mut gaps, mut m) = (vec![], true, 0, map);
    for _ in 0..unit {
        if m & 0xF != 0 {
            offsets.push(idx + (m & 0xF) as usize - 1 + gaps);
            first = false;
        } else if !first {
            gaps += 1;
        }
        m >>= 4;
    }
    let n = offsets.len();
    if data.len() % n != 0 {
        return Err(format!("part length {} not a multiple of {n} bytes per unit", data.len()));
    }
    let count = data.len() / n;
    let end = (cursors[idx] + count) * unit;
    if image.len() < end {
        image.resize(end, 0);
    }
    for k in 0..count {
        let base = (cursors[idx] + k) * unit;
        for (j, off) in offsets.iter().enumerate() {
            image[base + off] = data[k * n + j];
        }
    }
    cursors[idx] += count;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn run(seq: &[(&[u8], usize, u64)]) -> String {
        let (mut image, mut cursors) = (vec![], [0; 8]);
        for (data, unit, map) in seq {
            place(&mut image, data, *unit, *map, &mut cursors).unwrap();
        }
        image.iter().map(|b| format!("{b:02x}")).collect()
    }

    // Expected values are rom_packer.place()'s output for the same calls.
    #[test]
    fn matches_rom_packer() {
        assert_eq!(run(&[(&[1, 2, 3], 2, 0x01), (&[4, 5, 6], 2, 0x10)]), "010402050306");
        assert_eq!(run(&[(&[1, 2, 3, 4], 2, 0x21)]), "01020304");
        assert_eq!(run(&[(&[1, 2], 4, 0x0102), (&[9, 9], 4, 0x0010), (&[7], 4, 0x1000)]), "0009000700090000");
        assert_eq!(run(&[(&[0x14, 6], 1, 1), (&[0x14, 6], 1, 1)]), "14061406");
        assert_eq!(run(&[(&[1, 2, 3, 4], 2, 0x01)]), "0100020003000400");
    }

    #[test]
    fn games_json_loads() {
        let t = table();
        assert_eq!(t.sets.len(), 35);
        assert!(t.sets.iter().all(|s| s.roms.iter().any(|r| r.index == BIOS_INDEX)));
    }
}
