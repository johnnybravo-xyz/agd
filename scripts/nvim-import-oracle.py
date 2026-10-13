#!/usr/bin/env python3
import argparse
import json
import os
import pathlib
import subprocess

ROOT = pathlib.Path(__file__).resolve().parent.parent


def run(case: pathlib.Path) -> subprocess.CompletedProcess[str]:
    environment = os.environ.copy()
    environment["AGD_FIXTURE_DIR"] = "/fixture"
    environment["AGD_IMPORT_SOURCE"] = "profile"
    return subprocess.run(
        [
            environment.get("NVIM", "nvim"),
            "--headless",
            "-u",
            str(case / "init.lua"),
            "-c",
            f"luafile {ROOT / 'agd/probes/nvim.lua'}",
            "-c",
            "qa!",
        ],
        text=True,
        stdout=subprocess.PIPE,
        stderr=subprocess.STDOUT,
        env=environment,
    )


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--update", action="store_true")
    arguments = parser.parse_args()
    root = ROOT / "conformance/import/nvim"
    failed = False
    for case in sorted(path.parent for path in root.glob("*/init.lua")):
        result = run(case)
        marker = "AGD_JSON:"
        payload = next(
            (line[len(marker):] for line in result.stdout.splitlines() if line.startswith(marker)),
            None,
        )
        if case.name == "not-setup":
            actual = "setup has not completed" if "setup has not completed" in result.stdout else result.stdout
            expected = case / "failure.txt"
            if arguments.update:
                expected.write_text(actual.rstrip() + "\n", encoding="utf-8")
                print(f"updated {expected}")
            elif not expected.is_file() or expected.read_text(encoding="utf-8") != actual.rstrip() + "\n":
                print("Neovim capability failure oracle drift")
                failed = True
            continue
        if result.returncode != 0 or payload is None:
            raise RuntimeError(f"Neovim probe failed for {case}: {result.stdout}")
        actual = json.loads(payload)
        editor_version = actual.get("editor_version", "")
        if len(editor_version.split(".")) != 3 or not all(
            part.isdigit() for part in editor_version.split(".")
        ):
            raise RuntimeError(f"Neovim probe returned invalid version: {editor_version}")
        expected = case / "resolved.json"
        if arguments.update:
            expected.write_text(
                json.dumps(actual, indent=2, sort_keys=True) + "\n",
                encoding="utf-8",
            )
            print(f"updated {expected}")
        elif not expected.is_file():
            print(f"Neovim import oracle fixture missing: {case.name}")
            failed = True
        else:
            normalized_actual = dict(actual)
            normalized_expected = json.loads(expected.read_text(encoding="utf-8"))
            normalized_actual.pop("editor_version", None)
            normalized_expected.pop("editor_version", None)
            if normalized_expected != normalized_actual:
                print(f"Neovim import oracle drift: {case.name}")
                failed = True
    return int(failed)


if __name__ == "__main__":
    raise SystemExit(main())
