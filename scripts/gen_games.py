#!/usr/bin/env python3
"""Make src/games.json, the game list built into the plugin, from the core's repository.

  scripts/gen_games.py /path/to/NamcoSystem11-Pocket

The sets, their titles and instance names come from the core's tools/make_games.py (which reads the MRAs in
references/SYSTEM11_MiSTer), and each set's ROM images are written out part by part in the order tools/rom_packer.py
reads them, so the plugin packs exactly what make_games.py packs. Run it again whenever the core's game list or MRAs
change, and commit the result.
"""
import argparse, json, sys
import xml.etree.ElementTree as ET
from pathlib import Path

OUT = Path(__file__).resolve().parents[1] / "src/games.json"


def parts(node, unit):
    """The parts of a <rom> with their unit length in bytes, as rom_packer.pack() walks them."""
    for p in node:
        if p.tag == "interleave":
            yield from parts(p, int(p.get("output", "8")) // 8)
        elif p.tag == "part":
            yield p, unit


def rom_of(rom):
    default_zip = rom.get("zip", "")
    out = []
    for p, unit in parts(rom, 1):
        e = {"unit": unit, "map": int(p.get("map", "0"), 16), "repeat": int(p.get("repeat", "1"), 0)}
        if p.get("name"):
            e["name"] = p.get("name")
            e["zips"] = [z.strip() for z in (p.get("zip") or default_zip).split("|") if z.strip()]
            if p.get("crc"):
                e["crc"] = int(p.get("crc"), 16)
            e["offset"] = int(p.get("offset", "0"), 0)
            e["length"] = int(p.get("length", "0"), 0)
        else:
            e["data"] = "".join((p.text or "").split()).lower()
            bytes.fromhex(e["data"])
        out.append(e)
    return {"index": int(rom.get("index", "0")), "parts": out}


def main():
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument("core", type=Path, help="the core's repository (with references/SYSTEM11_MiSTer checked out)")
    a = ap.parse_args()
    sys.path.insert(0, str(a.core.resolve() / "tools"))
    import make_games

    data_json = a.core / "pocket/dist/Cores" / make_games.CORE / "data.json"
    slots = json.loads(data_json.read_text())["data"]["data_slots"]
    bios = next(s for s in slots if s["id"] == 8)

    sets = []
    for s in make_games.sets():
        roms = [rom_of(r) for r in ET.parse(s["mra"]).getroot().findall("rom")]
        indices = sorted({r["index"] for r in roms})
        if 8 not in indices:
            sys.exit(f"{s['set']}: no C76 BIOS (index 8) in its MRA")
        if [i for i in indices if i != 8 and i not in make_games.SLOT_OF]:
            sys.exit(f"{s['set']}: an index has no data slot in make_games.SLOT_OF")
        sets.append({"set": s["set"], "title": s["title"], "alt": s["alt"],
                     "instance": make_games.instance_name(s), "roms": roms})

    table = {
        "platform": make_games.PLATFORM,
        "core": make_games.CORE,
        "bank": make_games.BANK,
        "slot_of": sorted([i, sid] for i, sid in make_games.SLOT_OF.items()),
        "bios": {"filename": bios["filename"], "md5": bios["md5"]},
        "sets": sets,
    }
    # One part per line, so a change to an MRA shows up as a readable diff.
    compact = {}
    for s in sets:
        for r in s["roms"]:
            for i, p in enumerate(r["parts"]):
                key = f"@part{len(compact)}@"
                compact[key] = json.dumps(p)
                r["parts"][i] = key
    text = json.dumps(table, indent=1)
    for key, value in compact.items():
        text = text.replace(f'"{key}"', value)
    OUT.write_text(text + "\n")
    print(f"{len(sets)} sets -> {OUT}")


if __name__ == "__main__":
    main()
