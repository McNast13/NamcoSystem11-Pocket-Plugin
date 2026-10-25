//! The game list (src/games.json, made from the core's MRAs by scripts/gen_games.py) and the port of the core's
//! tools/rom_packer.py that turns a set's MAME zips into its ROM images.

use crate::zip::Zip;
use serde::Deserialize;
use std::collections::{BTreeMap, BTreeSet, HashMap};
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
    /// The zips the shared C76 BIOS comes from.
    pub fn bios_zips(&self) -> Vec<&str> {
        self.roms.iter().filter(|r| r.index == BIOS_INDEX).flat_map(|r| r.parts.iter()).flat_map(|p| p.zips.iter())
            .map(|z| z.as_str()).collect()
    }

    /// The named files of the game itself (not the shared BIOS).
    fn game_parts(&self) -> impl Iterator<Item = &Part> {
        self.roms.iter().filter(|r| r.index != BIOS_INDEX).flat_map(|r| r.parts.iter()).filter(|p| p.name.is_some())
    }

    fn bios_parts(&self) -> impl Iterator<Item = &Part> {
        self.roms.iter().filter(|r| r.index == BIOS_INDEX).flat_map(|r| r.parts.iter()).filter(|p| p.name.is_some())
    }
}

/// Every zip in one folder (the SD card's Assets/<platform>/common) and its subfolders, and every zip inside those
/// zips. A set's files are looked for in the zips it names first, as make_games.py does, and then by CRC in all the
/// others, so renamed zips, zips from older MAME versions (where a clone's files were in another set's zip) and
/// zips of zips all work.
pub struct ZipSet {
    zips: Vec<Zip>,
    /// The zip each nested zip was found in.
    parent: Vec<Option<usize>>,
    /// Lowercase file name -> zip, the first found (zips on the card before zips inside zips).
    by_name: HashMap<String, usize>,
    /// Zips that can't be read, with why.
    pub unreadable: Vec<String>,
    /// The zips a file has been found in.
    used: BTreeSet<usize>,
}

/// How deep below the folder to look, so a user can copy in a folder of zips (or a folder of folders).
const MAX_DEPTH: usize = 4;

/// A folder's own zips, then its subfolders' in name order.
fn walk(dir: &Path, depth: usize, files: &mut Vec<PathBuf>) {
    let Ok(list) = std::fs::read_dir(dir) else { return };
    let mut list: Vec<_> = list.flatten().filter(|f| !f.file_name().to_string_lossy().starts_with('.')).collect();
    list.sort_by_key(|f| f.file_name());
    let mut subdirs = vec![];
    for f in list {
        let path = f.path();
        if path.is_dir() {
            subdirs.push(path);
        } else if f.file_name().to_string_lossy().to_lowercase().ends_with(".zip") {
            files.push(path);
        }
    }
    if depth < MAX_DEPTH {
        for d in subdirs {
            walk(&d, depth + 1, files);
        }
    }
}

fn lower_file_name(name: &str) -> String { name.rsplit('/').next().unwrap_or(name).to_lowercase() }

impl ZipSet {
    pub fn new(dir: &Path) -> ZipSet {
        let mut files = vec![];
        walk(dir, 0, &mut files);
        let mut set =
            ZipSet { zips: vec![], parent: vec![], by_name: HashMap::new(), unreadable: vec![], used: BTreeSet::new() };
        let mut nested = vec![];
        for path in files {
            let label = path.strip_prefix(dir).unwrap_or(&path).display().to_string();
            match Zip::open(path.clone(), label) {
                Ok(zip) => {
                    let i = set.zips.len();
                    for e in zip.entries.iter().filter(|e| e.name.to_lowercase().ends_with(".zip")) {
                        let label = format!("{} > {}", zip.label, e.name);
                        match zip.read(e).and_then(|bytes| Zip::from_bytes(bytes, label)) {
                            Ok(inner) => nested.push((i, lower_file_name(&e.name), inner)),
                            Err(err) => set.unreadable.push(err),
                        }
                    }
                    let name = path.file_name().unwrap_or_default().to_string_lossy().to_lowercase();
                    set.by_name.entry(name).or_insert(i);
                    set.zips.push(zip);
                    set.parent.push(None);
                }
                Err(err) => set.unreadable.push(err),
            }
        }
        for (parent, name, zip) in nested {
            set.by_name.entry(name).or_insert(set.zips.len());
            set.zips.push(zip);
            set.parent.push(Some(parent));
        }
        set
    }

    /// The zips on the card (not counting zips inside them).
    pub fn files(&self) -> usize { self.parent.iter().filter(|p| p.is_none()).count() }

    /// The zips on the card that some file was found in (in them, or in a zip inside them), and the others.
    pub fn used_and_unused(&self) -> (Vec<String>, Vec<String>) {
        let (mut used, mut unused) = (vec![], vec![]);
        for (i, zip) in self.zips.iter().enumerate().filter(|(i, _)| self.parent[*i].is_none()) {
            let hit = self.used.iter().any(|&u| u == i || self.parent[u] == Some(i));
            if hit { used.push(zip.label.clone()) } else { unused.push(zip.label.clone()) }
        }
        (used, unused)
    }

    fn hit(&mut self, zip: usize, entry: Option<usize>) -> Option<(usize, usize)> {
        let entry = entry?;
        self.used.insert(zip);
        Some((zip, entry))
    }

    /// Where a part is: (zip, member). First in the zips the MRA names, as rom_packer.ZipSet.read() does (a member
    /// with the part's name must have its CRC; otherwise any member with the part's CRC will do); then in every
    /// other zip, by CRC (or by name, for a part the MRA gives no CRC for).
    fn find(&mut self, part: &Part) -> Result<(usize, usize), String> {
        let name = part.name.as_deref().unwrap_or_default();
        let mut wrong_crc = None;
        for z in &part.zips {
            let Some(&i) = self.by_name.get(&z.to_lowercase()) else { continue };
            let zip = &self.zips[i];
            let named = zip.entries.iter().position(|e| e.name == name);
            match (named, part.crc) {
                (Some(e), Some(crc)) if zip.entries[e].crc != crc => {
                    wrong_crc.get_or_insert(format!(
                        "{}: {name} has CRC {:08x}, the core needs {crc:08x} (a romset from another MAME version?)",
                        zip.label, zip.entries[e].crc
                    ));
                }
                (Some(e), _) => {
                    self.used.insert(i);
                    return Ok((i, e));
                }
                (None, Some(crc)) => {
                    if let Some(found) = self.hit(i, zip.entries.iter().position(|e| e.crc == crc)) {
                        return Ok(found);
                    }
                }
                (None, None) => {}
            }
        }
        for i in 0..self.zips.len() {
            let zip = &self.zips[i];
            let e = match part.crc {
                Some(crc) => zip.entries.iter().position(|e| e.crc == crc),
                None => zip.entries.iter().position(|e| lower_file_name(&e.name) == name.to_lowercase()),
            };
            if let Some(found) = self.hit(i, e) {
                return Ok(found);
            }
        }
        Err(wrong_crc.unwrap_or_default())
    }

    /// Which of these parts can't be found: (name, why) for each, from the zips' directories (nothing is unpacked).
    fn missing<'a>(&mut self, parts: impl Iterator<Item = &'a Part>) -> (usize, Vec<(String, String)>) {
        let (mut found, mut missing) = (0, vec![]);
        for part in parts {
            match self.find(part) {
                Ok(_) => found += 1,
                Err(why) => missing.push((part.name.clone().unwrap_or_default(), why)),
            }
        }
        (found, missing)
    }

    /// Whether the shared C76 BIOS can be found.
    pub fn has_bios(&mut self, set: &Set) -> bool { self.missing(set.bios_parts()).1.is_empty() }

    /// Whether every file of a set's game is in the zips with the right CRC: None if none of them are (the set's
    /// romset isn't here at all), otherwise Ok or what is missing.
    pub fn check(&mut self, set: &Set) -> Option<Result<(), String>> {
        let (found, missing) = self.missing(set.game_parts());
        if found == 0 {
            return None;
        }
        if missing.is_empty() {
            return Some(Ok(()));
        }
        if let Some((_, why)) = missing.iter().find(|(_, why)| !why.is_empty()) {
            return Some(Err(why.clone()));
        }
        let names: Vec<&str> = missing.iter().map(|(n, _)| n.as_str()).collect();
        let shown = if names.len() > 3 { format!("{}, ...", names[..3].join(", ")) } else { names.join(", ") };
        Some(Err(format!("{} of its {} files missing: {shown}", names.len(), found + names.len())))
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
                            Ok((z, e)) => self.zips[z].read(&self.zips[z].entries[e])?,
                            Err(why) if why.is_empty() => {
                                return Err(format!("{} is not in any zip", part.name.as_deref().unwrap_or_default()))
                            }
                            Err(why) => return Err(why),
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
