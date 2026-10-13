#!/usr/bin/env python3
import argparse
import json
import os
import pathlib
import subprocess
import tempfile

ROOT = pathlib.Path(__file__).resolve().parent.parent
COMMIT = "359538284a21c1edac20cb8a0b6b772ca098098e"


def main() -> None:
    parser = argparse.ArgumentParser()
    parser.add_argument("--nvim", default="target/nvim-0.12.6/bin/nvim")
    parser.add_argument("--plugin", default="target/nvim-orgmode-0.7.5")
    arguments = parser.parse_args()
    nvim = pathlib.Path(arguments.nvim).resolve()
    plugin = pathlib.Path(arguments.plugin).resolve()
    with tempfile.TemporaryDirectory(prefix="agendum-nvim-") as directory:
        directory = pathlib.Path(directory)
        agenda = directory / "tasks.org"
        agenda.write_text("* TODO Actual plugin probe\n", encoding="utf-8")
        init = directory / "init.lua"
        init.write_text(
            "vim.opt.runtimepath:prepend(" + json.dumps(str(plugin)) + ")\n"
            "require('orgmode').setup({\n"
            "  org_agenda_files = { " + json.dumps(str(agenda)) + " },\n"
            "  org_default_notes_file = " + json.dumps(str(agenda)) + ",\n"
            "  org_todo_keywords = { 'TODO(t)', 'NEXT(n!)', '|', 'DONE(d!)' },\n"
            "})\n",
            encoding="utf-8",
        )
        environment = os.environ.copy()
        environment["AGENDUM_IMPORT_SOURCE"] = "profile"
        result = subprocess.run(
            [str(nvim), "--headless", "-u", str(init), "-l", str(ROOT / "agd/probes/nvim.lua")],
            check=True,
            text=True,
            stdout=subprocess.PIPE,
            stderr=subprocess.STDOUT,
            env=environment,
        )
    marker = "AGENDUM_JSON:"
    payload = next(
        (line[len(marker):] for line in result.stdout.splitlines() if line.startswith(marker)),
        None,
    )
    if payload is None:
        raise RuntimeError(f"actual nvim-orgmode probe marker missing: {result.stdout}")
    resolved = json.loads(payload)
    if not resolved["editor_version"].startswith("0.12."):
        raise RuntimeError(f"unexpected Neovim version: {resolved['editor_version']}")
    if resolved["org_version"] != f"0.7.5@{COMMIT}":
        raise RuntimeError(f"unexpected nvim-orgmode identity: {resolved['org_version']}")
    print(f"Neovim {resolved['editor_version']} with nvim-orgmode {resolved['org_version']}")


if __name__ == "__main__":
    main()
