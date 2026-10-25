#!/usr/bin/env python3
"""Run the plugin (plugin.wasm) headless, the way Pocket Sync and pupdate do: the SD card folder is the plugin's
pocket/, each Choice is answered from --answer in order (default: the first choice), and the plugin's log goes to
stdout. Needs the extism Python host (python3 -m pip install extism).

  python3 host/run_plugin.py target/wasm32-wasip1/release/plugin.wasm /path/to/sd-card --answer "Install all"

Adapted from the PlayStation game files plugin's host/run_plugin.py (github.com/GuardianSoul/openfpga-PlayStation-plugin).
"""
import argparse
import json
import sys
from pathlib import Path

import extism


@extism.host_fn(name="print_msg", signature=([extism.ValType.I64], [extism.ValType.I64]))
def print_msg(plugin, inputs, outputs):
    sys.stdout.write(plugin.input_string(inputs[0]))
    sys.stdout.flush()


@extism.host_fn(name="open_url", signature=([extism.ValType.I64], [extism.ValType.I64]))
def open_url(plugin, inputs, outputs):
    print(f"[open_url {plugin.input_string(inputs[0])}]")


def main():
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument("wasm", type=Path)
    ap.add_argument("card", type=Path)
    ap.add_argument("--answer", action="append", default=[], help="choice to pick, in order (matched by its start)")
    a = ap.parse_args()
    manifest = {"wasm": [{"path": str(a.wasm.resolve())}], "allowed_paths": {str(a.card.resolve()): "pocket"}}
    answers = list(a.answer)
    with extism.Plugin(manifest, wasi=True, functions=[print_msg, open_url]) as plugin:
        message = json.loads(plugin.call("start", b""))
        while message != "Exit" and "Exit" not in message:
            (kind, body), = message.items()
            if kind != "Choice":
                raise SystemExit(f"unexpected {kind} message: {body}")
            want = answers.pop(0) if answers else body["choices"][0]
            value = next((c for c in body["choices"] if c.startswith(want)), None)
            if value is None:
                raise SystemExit(f"{want!r} is not one of {body['choices']}")
            print("  choices: " + " | ".join(body["choices"]))
            print(f"[{body['query']}: {value}]")
            reply = {"Answer": {"name": body["name"], "value": value}}
            message = json.loads(plugin.call("handle_response", json.dumps(reply).encode()))


if __name__ == "__main__":
    main()
