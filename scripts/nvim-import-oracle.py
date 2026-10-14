#!/usr/bin/env python3
"""Runs agd's Neovim config reader on each conformance/import/nvim case and
compares its payload with the case's resolved.json (or failure.txt).

The reader runs in `nvim --clean`, as `agd setup import` does, over the
case's .lua files that mention orgmode. Use --update to rewrite the
expected files."""
import argparse
import json
import os
import pathlib
import subprocess

ROOT = pathlib.Path(__file__).resolve().parent.parent
READER = ROOT / "agd/readers/nvim.lua"


def run(case: pathlib.Path) -> subprocess.CompletedProcess[str]:
    files = sorted(
        path for path in case.rglob("*.lua") if "orgmode" in path.read_text(encoding="utf-8")
    )
    environment = os.environ.copy()
    environment["AGD_FIXTURE_DIR"] = "/fixture"
    environment["AGD_IMPORT_FILES"] = "\n".join(str(path) for path in files)
    environment["AGD_IMPORT_ROOT"] = str(case)
    return subprocess.run(
        [environment.get("NVIM", "nvim"), "--clean", "--headless", "-l", str(READER)],
        text=True,
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE,
        env=environment,
    )


def check(case: pathlib.Path, update: bool) -> bool:
    result = run(case)
    failure = case / "failure.txt"
    if failure.exists():
        actual = result.stderr.strip().splitlines()[0] if result.returncode else "succeeded"
        if update:
            failure.write_text(actual + "\n", encoding="utf-8")
            print(f"updated {failure}")
            return True
        return failure.read_text(encoding="utf-8") == actual + "\n"
    marker = "AGD_JSON:"
    payload = next(
        (line[len(marker):] for line in result.stdout.splitlines() if line.startswith(marker)),
        None,
    )
    if result.returncode != 0 or payload is None:
        raise RuntimeError(f"Neovim reader failed for {case}: {result.stderr}")
    actual = json.loads(payload)
    # The pinned Neovim in CI and a local one differ only in this field.
    actual.pop("editor_version")
    expected = case / "resolved.json"
    if update:
        expected.write_text(json.dumps(actual, indent=2, sort_keys=True) + "\n", encoding="utf-8")
        print(f"updated {expected}")
        return True
    return expected.is_file() and json.loads(expected.read_text(encoding="utf-8")) == actual


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--update", action="store_true")
    arguments = parser.parse_args()
    failed = False
    for case in sorted(path for path in (ROOT / "conformance/import/nvim").iterdir() if path.is_dir()):
        if not check(case, arguments.update):
            print(f"Neovim import oracle drift: {case.name}")
            failed = True
    return int(failed)


if __name__ == "__main__":
    raise SystemExit(main())
