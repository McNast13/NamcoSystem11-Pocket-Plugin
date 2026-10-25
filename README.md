# Namco System 11 games plugin

A [Pocket plugin](https://github.com/openfpga-library/pocket-plugin) for Pocket Sync and pupdate. It installs the games for
the [Namco System 11 core](https://github.com/McNast13/NamcoSystem11-Pocket) from your own MAME romsets, the same way the
core's `tools/make_games.py --roms ... --sd ...` does, without needing Python or a clone of the core's repository.

No ROMs are included or downloaded: it only uses the zips you put on the SD card.

## Use

1. Install the Namco System 11 core.
2. Copy your **current MAME** romset zips (for example `tekken.zip`, `souledge.zip`) and `namcoc76.zip` (the C76 sound
   CPU BIOS every game needs) into `Assets/namco_system11/common/` on the SD card. A folder of zips works too: the
   plugin looks in subfolders (up to four levels down).
3. In Pocket Sync or pupdate, add a plugin from this repository's link, put the SD card in and open
   *Namco System 11 games*.

The plugin finds each game's files by their content, so the zips' names don't matter: renamed zips
(`tekken2 (1).zip`), zips from older MAME versions (where a clone's files were in another set's zip, or had other
names), merged sets, and zips inside zips (a download that holds `tekken2.zip` and `namcoc76.zip`) all work. It looks
in the zip named after the set first, as `make_games.py` does, then for a file with the right CRC in every other zip.

It then lists, for every game in the core's list (35 sets, main versions and alternatives) that any of the zips has
files for, whether it can be installed:

- **Ready**: every file the game needs was found with the right CRC.
- **Can't be installed**: which files are missing, or which file has the wrong CRC (usually a romset from another
  MAME version).
- Zips with no files for any game in the list are named and ignored, and `namcoc76.zip` is reported once if no zip
  has the C76 BIOS.

Choose *Install all* (or *Install the main versions only*) and it writes, for each game,
`Assets/namco_system11/common/<set>/rom_<n>.bin` and the game's entry in the core's list with each file's MD5, plus
`common/c76.bin`. Then open Namco System 11 on the Pocket and pick the game. Your zips are left where they are; once the
games are installed you can delete them.

Every file is written as `<name>.part` first and renamed only when it is complete, so a run that is stopped part-way
never leaves a half-written file under its real name; the next run removes any `.part` files left behind.

## How it works

- `src/games.json` is the game list: every set's title, entry name, and ROM images part by part (zip, file name, CRC,
  interleave map), made from the core's MRAs by `scripts/gen_games.py`, which uses the core's own `make_games.py` for the
  set list and entry names. Regenerate it when the core's game list changes:
  ```
  scripts/gen_games.py /path/to/NamcoSystem11-Pocket
  ```
- `src/pack.rs` is a port of the core's `tools/rom_packer.py` (the MiSTer MRA loader's interleaving), and `src/lib.rs`
  of `make_games.py`'s install step (data slots, the 16 MB bank split into `rom_3b.bin`, config padding, instance files).
- `src/zip.rs` reads zips (stored or deflated), and zips inside zips (held in memory). Checking a set only reads the
  zips' directories; files are unpacked, and their CRCs checked again, when a game is installed.

## Build

```
rustup target add wasm32-wasip1
scripts/build.sh               # target/wasm32-wasip1/release/plugin.wasm
cargo test --lib --target "$(rustc -vV | sed -n 's/host: //p')"
```

## Test against make_games.py

`host/run_plugin.py` (needs `pip install extism`) runs the plugin headless on any folder, as Pocket Sync and pupdate do:

```
python3 host/run_plugin.py target/wasm32-wasip1/release/plugin.wasm /path/to/sd-card --answer "Install all"
```

`tests/compare_with_make_games.py` installs the same zips with `make_games.py` and with the plugin, and checks every
file is identical. The plugin runs twice: on the zips as they are, and on the zips renamed and packed into one zip of
zips in a subfolder.

```
python3 tests/compare_with_make_games.py /path/to/NamcoSystem11-Pocket /path/to/roms
```

## Release

Pocket Sync and pupdate use the latest release of this repository, which must carry `plugin.wasm` and `plugin.json` (the
release title is the version). Publishing a release runs `.github/workflows/release.yml`, which runs the unit tests,
builds `plugin.wasm` and attaches both files.

## Use of AI

This plugin was written with Claude (Anthropic's AI model) through Claude Code, under McNast13's direction, following
the design of the [PlayStation game files plugin](https://github.com/GuardianSoul/openfpga-PlayStation-plugin) (whose
`host/run_plugin.py` this one's is adapted from). Its output is checked file for file against the core's
`make_games.py`.

## License

GPL-3.0-only (see [LICENSE](LICENSE)).
