#!/usr/bin/env python3
"""Runs agd's Emacs config reader on each conformance/import/emacs case in
the pinned Emacs and compares its payload with the case's resolved.json.

The reader reads the case's init.el without loading it, as `agd setup
import` does. Use --update to rewrite the expected files."""
import argparse
import json
import pathlib
import subprocess
import uuid

IMAGE = "silex/emacs:30.2-alpine@sha256:2dd43b22af2e2592753ad92c3cb78fc580ce5981fc4d8ad1edee60ec92241ae5"
ROOT = pathlib.Path(__file__).resolve().parent.parent


def resolved(case: pathlib.Path) -> dict:
    name = f"agd-import-{uuid.uuid4().hex}"
    command = [
        "docker",
        "create",
        "--name",
        name,
        "--env",
        "AGD_FIXTURE_DIR=/fixture",
        "--env",
        "AGD_IMPORT_FILES=/fixture/init.el",
        IMAGE,
        "emacs",
        "-Q",
        "--batch",
        "-l",
        "/reader.el",
    ]
    try:
        subprocess.run(command, check=True, stdout=subprocess.DEVNULL)
        subprocess.run(
            ["docker", "cp", str(case) + "/.", f"{name}:/fixture"],
            check=True,
            stdout=subprocess.DEVNULL,
        )
        subprocess.run(
            ["docker", "cp", str(ROOT / "agd/readers/emacs.el"), f"{name}:/reader.el"],
            check=True,
            stdout=subprocess.DEVNULL,
        )
        output = subprocess.run(
            ["docker", "start", "-a", name],
            check=True,
            text=True,
            stdout=subprocess.PIPE,
        ).stdout
    finally:
        subprocess.run(
            ["docker", "rm", "-f", name],
            stdout=subprocess.DEVNULL,
            stderr=subprocess.DEVNULL,
        )
    marker = "AGD_JSON:"
    payload = next((line[len(marker):] for line in output.splitlines() if line.startswith(marker)), None)
    if payload is None:
        raise RuntimeError(f"reader marker missing for {case}")
    return json.loads(payload)


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--update", action="store_true")
    arguments = parser.parse_args()
    root = ROOT / "conformance/import/emacs"
    failed = False
    for case in sorted(path.parent for path in root.glob("*/init.el")):
        actual = json.dumps(resolved(case), indent=2, sort_keys=True) + "\n"
        expected = case / "resolved.json"
        if arguments.update:
            expected.write_text(actual, encoding="utf-8")
            print(f"updated {expected}")
        elif not expected.is_file() or expected.read_text(encoding="utf-8") != actual:
            print(f"Emacs import oracle drift: {case.name}")
            failed = True
    return int(failed)


if __name__ == "__main__":
    raise SystemExit(main())
