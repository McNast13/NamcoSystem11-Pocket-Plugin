#!/usr/bin/env python3
"""Check the plugin installs exactly what the core's make_games.py does, file for file, from the same zips.

  tests/compare_with_make_games.py /path/to/NamcoSystem11-Pocket /path/to/roms [--wasm plugin.wasm]

Builds SD cards in a temporary folder: one with `make_games.py --roms ROMS --sd A`, and two with the core's Cores/
folder, on which the plugin is run (answering "Install all"): one with the zips as they are in
Assets/namco_system11/common/, and one with every zip renamed and all of them packed into one zip of zips in a
subfolder (the plugin finds files by CRC). Every file under Assets/ (zips aside) must be the same as make_games.py's.
Needs the extism Python host (pip install extism).
"""
import argparse, filecmp, shutil, subprocess, sys, tempfile, zipfile
from pathlib import Path

HERE = Path(__file__).resolve().parents[1]


def tree(root):
    return {p.relative_to(root) for p in root.rglob("*") if p.is_file() and p.suffix.lower() != ".zip"}


def main():
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument("core", type=Path)
    ap.add_argument("roms", type=Path)
    ap.add_argument("--wasm", type=Path, default=HERE / "target/wasm32-wasip1/release/plugin.wasm")
    a = ap.parse_args()
    with tempfile.TemporaryDirectory() as tmp:
        tmp = Path(tmp)
        ref = tmp / "make_games"
        subprocess.run([sys.executable, a.core / "tools/make_games.py", "--roms", a.roms, "--sd", ref], check=True)

        if not tree(ref / "Assets"):
            sys.exit("make_games.py installed nothing: no usable zips in " + str(a.roms))
        problems = []
        for kind in ("as they are", "renamed, in a zip of zips"):
            card = tmp / kind.replace(" ", "_").replace(",", "")
            shutil.copytree(a.core / "pocket/dist/Cores", card / "Cores")
            common = card / "Assets/namco_system11/common"
            common.mkdir(parents=True)
            zips = sorted(a.roms.glob("*.zip"))
            if kind == "as they are":
                for z in zips:
                    shutil.copy(z, common)
            else:
                (common / "sub").mkdir()
                with zipfile.ZipFile(common / "sub/all my roms.zip", "w") as outer:
                    for n, z in enumerate(zips):
                        outer.write(z, f"renamed {n}.zip")
            print(f"== plugin, zips {kind}")
            subprocess.run([sys.executable, HERE / "host/run_plugin.py", a.wasm, card, "--answer", "Install all"], check=True)
            want, got = tree(ref / "Assets"), tree(card / "Assets")
            problems += [f"{kind}: only from make_games.py: {p}" for p in sorted(want - got)]
            problems += [f"{kind}: only from the plugin: {p}" for p in sorted(got - want)]
            problems += [f"{kind}: differs: {p}" for p in sorted(want & got)
                         if not filecmp.cmp(ref / "Assets" / p, card / "Assets" / p, shallow=False)]
            print(f"{kind}: {len(want & got)} files compared")
        for p in problems:
            print(p)
        print(f"{len(problems)} problem(s)")
        sys.exit(1 if problems else 0)


if __name__ == "__main__":
    main()
