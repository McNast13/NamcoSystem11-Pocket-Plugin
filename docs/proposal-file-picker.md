# Proposal: a `Files` prompt, so a plugin can ask the user to pick files on their computer

## Why

Plugins can read and write the SD card (`pocket/`) and their own folder on the computer (`host/`), and can ask the
user for a choice or some text. Nothing lets the user give a plugin a file from elsewhere on their computer. So
plugins that work on the user's own files ask them to copy the files onto the card by hand first:

- [Namco System 11 games](https://github.com/McNast13/NamcoSystem11-Pocket) (in progress) turns the user's MAME
  romset zips into the core's game files. Today: "copy your zips into `Assets/namco_system11/common/`, then run the
  plugin."
- [PlayStation game files](https://github.com/GuardianSoul/openfpga-PlayStation-plugin): "put your games in
  `Assets/playstation/common/`, each in its own folder."

Asking for a path in a `Text` prompt doesn't help: the plugin can only open paths under `pocket/` and `host/`.

## Proposal

A fourth `PluginMessage`:

```rust
enum PluginMessage {
    Choice { name: String, query: String, choices: Vec<String> },
    Text { name: String, query: String },
    Files {
        name: String,
        query: String,
        extensions: Vec<String>, // e.g. ["zip"]; empty = any file
        multiple: bool,          // allow choosing more than one file
        destination: String,     // where the host copies them, e.g. "pocket/Assets/namco_system11/common"
    },
    Exit,
}
```

```json
{"Files": {"name": "zips", "query": "Choose your MAME romset zips", "extensions": ["zip"], "multiple": true,
           "destination": "pocket/Assets/namco_system11/common"}}
```

The host:

1. Shows a file picker. In Pocket Sync this is the system's open dialog. In pupdate it could be a prompt for one or
   more paths (dragging files onto a terminal pastes their paths).
2. Copies the chosen files into `destination`, which must be below `pocket/` or `host/` (the host refuses anything
   else and doesn't follow `..`). It shows progress, and doesn't overwrite an existing file unless the user agrees.
3. Answers with the copied files' paths, as JSON in the usual `Answer`, relative to the plugin's view:

   ```json
   {"Answer": {"name": "zips", "value": "[\"pocket/Assets/namco_system11/common/tekken.zip\"]"}}
   ```

   `"[]"` means the user cancelled or chose nothing.

Why the host copies rather than the plugin reading the files in place:

- WASI folders are fixed when the plugin is loaded, so a host would otherwise have to reload the plugin or add a
  file-reading `host_fn`. Copying keeps plugins to plain file access.
- The plugin only ever sees files the user chose, in a place the user can see. It can't browse their computer.
- The host does the big copy natively, with its own progress and error handling. This matters for large files: a
  PlayStation disc image is several hundred MB, and a single-threaded WASM plugin would show no progress while copying.

## Telling a plugin the host supports it

A host that doesn't know `Files` would fail on it, so a plugin should only send it when the host says it can. The
host would set an Extism config value (plugins read it with `config::get`), which older hosts simply don't set:

```
pocket_plugin_features = "files"      (comma-separated, for future additions)
```

A plugin then offers both ways:

```rust
let can_pick = config::get("pocket_plugin_features")?
    .is_some_and(|f| f.split(',').any(|x| x.trim() == "files"));
let mut choices = vec![];
if can_pick {
    choices.push("Choose zips on this computer".into());
}
choices.push("Use the zips already on the SD card".into());
```

This also gives later additions somewhere to go without breaking older hosts.

## Questions

- Should `destination` be limited to `pocket/` (the common case), or also allow `host/` (staging on the computer
  first, useful when the card is nearly full)?
- Should the host ask before overwriting, or should the plugin say in `Files` whether overwriting is OK?
- Is choosing a folder worth having too (e.g. `folder: true`), for games kept as folders of files?

Happy to help with a draft PR to the spec and the demo host.
