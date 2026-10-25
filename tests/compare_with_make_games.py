#!/usr/bin/env python3
"""Check the plugin installs exactly what the core's make_games.py does, file for file, from the same zips.

  tests/compare_with_make_games.py /path/to/NamcoSystem11-Pocket /path/to/roms [--wasm plugin.wasm]

Builds two SD cards in a temporary folder: one with `make_games.py --roms ROMS --sd A`, and one with the core's
Cores/ folder and the zips in Assets/namco_system11/common/, on which the plugin is run (answering "Install all").
Every file under Assets/ (zips aside) must be the same on both. Needs the extism Python host (pip install extism).
"""
import argparse, filecmp, shutil, subprocess, sys, tempfile
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
        ref, card = tmp / "make_games", tmp / "plugin"
        subprocess.run([sys.executable, a.core / "tools/make_games.py", "--roms", a.roms, "--sd", ref], check=True)

        shutil.copytree(a.core / "pocket/dist/Cores", card / "Cores")
        common = card / "Assets/namco_system11/common"
        common.mkdir(parents=True)
        for z in a.roms.glob("*.zip"):
            shutil.copy(z, common)
        subprocess.run([sys.executable, HERE / "host/run_plugin.py", a.wasm, card, "--answer", "Install all"], check=True)

        want, got = tree(ref / "Assets"), tree(card / "Assets")
        problems = [f"only from make_games.py: {p}" for p in sorted(want - got)]
        problems += [f"only from the plugin: {p}" for p in sorted(got - want)]
        problems += [f"differs: {p}" for p in sorted(want & got)
                     if not filecmp.cmp(ref / "Assets" / p, card / "Assets" / p, shallow=False)]
        if not want:
            problems.append("make_games.py installed nothing: no usable zips in " + str(a.roms))
        for p in problems:
            print(p)
        print(f"{len(want & got)} files compared, {len(problems)} problem(s)")
        sys.exit(1 if problems else 0)


if __name__ == "__main__":
    main()
