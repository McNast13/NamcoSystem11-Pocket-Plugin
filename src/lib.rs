// Pocket Sync / pupdate plugin (openfpga-library/pocket-plugin): installs the games for the Namco System 11 core
// from the user's own MAME zips on the SD card, as the core's tools/make_games.py --roms --sd does.

use extism_pdk::*;
use md5::{Digest, Md5};
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::{Path, PathBuf};

mod pack;
mod zip;
use pack::{Set, Table, ZipSet, BIOS_INDEX};

const CARD: &str = "pocket";
const INSTALL_ALL: &str = "Install all";
const INSTALL_MAIN: &str = "Install the main versions only";
const EXIT: &str = "Exit";

#[derive(Clone, Serialize, ToBytes)]
#[encoding(Json)]
enum PluginMessage {
    Choice { name: String, query: String, choices: Vec<String> },
    #[allow(dead_code)] // part of the host protocol; this plugin never asks for text
    Text { name: String, query: String },
    Exit,
}

#[derive(Clone, Deserialize, FromBytes)]
#[encoding(Json)]
enum HostMessage {
    Answer { name: String, value: String },
    Kill,
}

#[host_fn]
extern "ExtismHost" {
    fn open_url(url: &str) -> ();
    fn print_msg(message: &str) -> ();
}

fn print(msg: &str) {
    unsafe {
        let _ = print_msg(msg);
    }
}

fn println(msg: &str) { print(&format!("{msg}\n")); }

fn common(t: &Table) -> PathBuf { Path::new(CARD).join("Assets").join(&t.platform).join("common") }

fn count(n: usize) -> String { if n == 1 { "1 game".into() } else { format!("{n} games") } }

fn label(s: &Set) -> String { format!("{} [{}]", s.title, s.set) }

/// The sets that can be installed from the zips on the card, after listing what was found.
fn scan(t: &Table, report: bool) -> Option<Vec<usize>> {
    let say = |m: &str| if report { println(m) };
    if !Path::new(CARD).join("Cores").join(&t.core).is_dir() {
        println(&format!("The Namco System 11 core ({}) is not on this SD card. Install the core first.", t.core));
        return None;
    }
    let dir = common(t);
    let shown = format!("Assets/{}/common", t.platform);
    remove_leftovers(t);
    let mut zips = ZipSet::new(&dir);
    if zips.files() == 0 {
        println(&format!("No zips in {shown} or its subfolders."));
        println(&format!(
            "Copy your own MAME romsets (current MAME, e.g. tekken.zip) and namcoc76.zip into {shown} (a folder of them is fine) and run this again."
        ));
        return None;
    }
    let has_bios = zips.has_bios(&t.sets[0]);
    let (mut ready, mut problems) = (vec![], vec![]);
    for (i, s) in t.sets.iter().enumerate() {
        match zips.check(s) {
            None => {}
            Some(Ok(())) => ready.push(i),
            Some(Err(e)) => problems.push(format!("  {}: {e}", label(s))),
        }
    }

    let (used, unused) = zips.used_and_unused();
    say(&format!("Found {} zip(s) in {shown}{}.", zips.files(), if used.is_empty() { String::new() } else {
        format!("; with files for this core: {}", used.join(", "))
    }));
    if !unused.is_empty() {
        let more = if unused.len() > 8 { format!(" and {} more", unused.len() - 8) } else { String::new() };
        say(&format!("No files for any game in the core's list (ignored): {}{more}", unused[..unused.len().min(8)].join(", ")));
    }
    for e in &zips.unreadable {
        say(&format!("Couldn't read {e}"));
    }
    if !ready.is_empty() {
        say(&format!("Ready to install{} ({}):", if has_bios { "" } else { " once the BIOS is there" }, ready.len()));
        for &i in &ready {
            let s = &t.sets[i];
            say(&format!("  {}{}", label(s), if s.alt { " (alternative version)" } else { "" }));
        }
    }
    if !problems.is_empty() {
        say(&format!("Can't be installed from these zips ({}):", problems.len()));
        for p in &problems {
            say(p);
        }
    }
    if !has_bios {
        println(&format!(
            "The C76 sound CPU BIOS that every game needs ({} in {}) isn't in any zip here: copy that zip in with the games.",
            t.bios.filename,
            t.sets[0].bios_zips().join(" or ")
        ));
        return None;
    }
    if ready.is_empty() {
        println("Nothing to install.");
        return None;
    }
    Some(ready)
}

fn menu(t: &Table) -> PluginMessage {
    let Some(ready) = scan(t, true) else { return PluginMessage::Exit };
    let main = ready.iter().filter(|&&i| !t.sets[i].alt).count();
    let mut choices = vec![format!("{INSTALL_ALL} ({})", count(ready.len()))];
    if main > 0 && main < ready.len() {
        choices.push(format!("{INSTALL_MAIN} ({})", count(main)));
    }
    choices.push(EXIT.into());
    PluginMessage::Choice { name: "install".into(), query: "Install these games on the SD card?".into(), choices }
}

/// Write a file under its real name only once it is complete: <name>.part first, then renamed.
fn write(path: &Path, data: &[u8]) -> Result<(), String> {
    use std::io::Write;
    let shown = || path.strip_prefix(CARD).unwrap_or(path).display().to_string();
    if let Some(dir) = path.parent() {
        fs::create_dir_all(dir).map_err(|e| format!("{}: {e}", shown()))?;
    }
    let part = PathBuf::from(format!("{}.part", path.display()));
    let result = (|| {
        let mut f = fs::File::create(&part)?;
        f.write_all(data)?;
        f.sync_all()?;
        drop(f);
        if fs::rename(&part, path).is_err() {
            // Some hosts don't rename over an existing file.
            let _ = fs::remove_file(path);
            fs::rename(&part, path)?;
        }
        Ok::<_, std::io::Error>(())
    })();
    result.map_err(|e| {
        let _ = fs::remove_file(&part);
        format!("{}: {e}", shown())
    })
}

/// Remove .part files a run that was stopped part-way left behind.
fn remove_leftovers(t: &Table) {
    let base = Path::new(CARD).join("Assets").join(&t.platform);
    let mut dirs = vec![common(t), base.join(&t.core), base.join(&t.core).join("_alternatives")];
    dirs.extend(t.sets.iter().map(|s| common(t).join(&s.set)));
    for dir in dirs {
        for f in fs::read_dir(dir).into_iter().flatten().flatten() {
            if f.file_name().to_string_lossy().ends_with(".part") {
                let _ = fs::remove_file(f.path());
            }
        }
    }
}

fn md5_hex(data: &[u8]) -> String { format!("{:x}", Md5::digest(data)) }

/// The instance file make_games.write_instance() writes (json.dumps(indent=2) + "\n").
fn instance_json(slots: &[(u32, String, String)]) -> String {
    let q = |s: &str| serde_json::to_string(s).unwrap();
    let slots: Vec<String> = slots
        .iter()
        .map(|(id, file, md5)| {
            format!("      {{\n        \"id\": {id},\n        \"filename\": {},\n        \"md5\": {}\n      }}", q(file), q(md5))
        })
        .collect();
    let list = if slots.is_empty() { "[]".to_string() } else { format!("[\n{}\n    ]", slots.join(",\n")) };
    format!("{{\n  \"instance\": {{\n    \"magic\": \"APF_VER_1\",\n    \"data_path\": \"\",\n    \"data_slots\": {list}\n  }}\n}}\n")
}

/// Pack one set and write its files and instance, as make_games.py does. Returns its C76 BIOS image.
fn install(t: &Table, zips: &mut ZipSet, s: &Set) -> Result<Vec<u8>, String> {
    let images = zips.pack(s)?;
    let dir = common(t).join(&s.set);
    let mut slots = vec![];
    for (&i, d) in &images {
        if i == BIOS_INDEX {
            continue;
        }
        let Some(&(_, slot)) = t.slot_of.iter().find(|(index, _)| *index == i) else {
            return Err(format!("ROM index {i} has no data slot"));
        };
        let mut d = d.clone();
        if i == 1 && d.len() % 4 != 0 {
            d.resize(d.len() + 4 - d.len() % 4, 0); // config: whole 32-bit words (the bridge delivers words)
        }
        // The banked ROM's data slot holds 16 MB; a larger one continues in rom_3b.bin, slot 5.
        let parts: Vec<(u32, String, &[u8])> = if i == 3 && d.len() > t.bank {
            vec![(slot, format!("rom_{i}.bin"), &d[..t.bank]), (5, "rom_3b.bin".into(), &d[t.bank..])]
        } else {
            vec![(slot, format!("rom_{i}.bin"), &d[..])]
        };
        for (slot, name, chunk) in parts {
            write(&dir.join(&name), chunk)?;
            slots.push((slot, format!("{}/{name}", s.set), md5_hex(chunk)));
        }
    }
    let inst = Path::new(CARD).join("Assets").join(&t.platform).join(&t.core).join(&s.instance);
    write(&inst, instance_json(&slots).as_bytes())?;
    Ok(images.get(&BIOS_INDEX).cloned().unwrap_or_default())
}

fn run(t: &Table, main_only: bool) -> PluginMessage {
    let Some(ready) = scan(t, false) else { return PluginMessage::Exit };
    let mut zips = ZipSet::new(&common(t));
    let (mut c76, mut done, mut failed) = (None::<Vec<u8>>, 0, 0);
    for i in ready.into_iter().filter(|&i| !main_only || !t.sets[i].alt) {
        let s = &t.sets[i];
        print(&format!("Installing {} ... ", label(s)));
        let result = install(t, &mut zips, s).and_then(|bios| match &c76 {
            Some(c) if *c != bios => Err("its C76 BIOS differs from the other games'".to_string()),
            _ => Ok(bios),
        });
        match result {
            Ok(bios) => {
                println("done");
                c76.get_or_insert(bios);
                done += 1;
            }
            Err(e) => {
                println(&format!("failed: {e}"));
                failed += 1;
            }
        }
    }
    if let Some(c76) = c76 {
        let md5 = md5_hex(&c76);
        if md5 != t.bios.md5 {
            println(&format!(
                "Warning: {} from namcoc76.zip has md5 {md5}, not the {} the core expects; the games may not have sound.",
                t.bios.filename, t.bios.md5
            ));
        }
        match write(&common(t).join(&t.bios.filename), &c76) {
            Ok(()) => println(&format!("Installed the C76 BIOS ({}).", t.bios.filename)),
            Err(e) => {
                println(&format!("Couldn't write the C76 BIOS: {e}"));
                failed += 1;
            }
        }
    }
    println(&format!("Installed {done} game(s){}.", if failed > 0 { format!(", {failed} problem(s) above") } else { String::new() }));
    if done > 0 {
        println("Open Namco System 11 on the Pocket and pick a game. Your zips are left in place; you can delete them.");
    }
    PluginMessage::Exit
}

#[plugin_fn]
pub fn start() -> FnResult<PluginMessage> {
    println("Namco System 11 games");
    Ok(menu(&pack::table()))
}

#[plugin_fn]
pub fn handle_response(input: HostMessage) -> FnResult<PluginMessage> {
    Ok(match input {
        HostMessage::Answer { name, value } if name == "install" && value.starts_with(INSTALL_ALL) => run(&pack::table(), false),
        HostMessage::Answer { name, value } if name == "install" && value.starts_with(INSTALL_MAIN) => run(&pack::table(), true),
        _ => PluginMessage::Exit,
    })
}
