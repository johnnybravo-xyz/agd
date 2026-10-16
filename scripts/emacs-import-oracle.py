#!/usr/bin/env python3
"""Runs agd's Emacs config reader on each conformance/import/emacs case in
the pinned Emacs and compares its payload with the case's resolved.json.

The reader reads the case's init.el without loading it, as `agd setup
import` does. Use --update to rewrite the expected files."""
import argparse
import json
import pathlib

from oracle import ROOT, agd_json, check, run_emacs


def resolved(case: pathlib.Path) -> dict:
    output = run_emacs(
        "/reader.el",
        {"AGD_FIXTURE_DIR": "/fixture", "AGD_IMPORT_FILES": "/fixture/init.el"},
        [(f"{case}/.", "/fixture"), (ROOT / "agd/readers/emacs.el", "/reader.el")],
    )
    payload = agd_json(output)
    if payload is None:
        raise RuntimeError(f"reader marker missing for {case}")
    return payload


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--update", action="store_true")
    arguments = parser.parse_args()
    root = ROOT / "conformance/import/emacs"
    failed = False
    for case in sorted(path.parent for path in root.glob("*/init.el")):
        actual = json.dumps(resolved(case), indent=2, sort_keys=True) + "\n"
        if not check(case / "resolved.json", actual, arguments.update):
            print(f"Emacs import oracle drift: {case.name}")
            failed = True
    return int(failed)


if __name__ == "__main__":
    raise SystemExit(main())
