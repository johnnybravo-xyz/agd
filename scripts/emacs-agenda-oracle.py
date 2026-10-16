#!/usr/bin/env python3
"""Writes or checks each conformance/agenda case's emacs.csv and emacs.txt:
pinned Emacs's org-batch-agenda-csv rows and org-batch-agenda text, byte for
byte. agd's --csv and text agenda must print the same."""
import argparse
import datetime
import pathlib
import subprocess
import sys
import tomllib
import uuid

IMAGE = "silex/emacs:30.2-alpine@sha256:2dd43b22af2e2592753ad92c3cb78fc580ce5981fc4d8ad1edee60ec92241ae5"


def docker(*args: str, capture: bool = False) -> str:
    result = subprocess.run(
        ["docker", *args],
        check=True,
        text=True,
        stdout=subprocess.PIPE if capture else subprocess.DEVNULL,
    )
    return result.stdout if capture else ""


def emacs_outputs(case_dir: pathlib.Path, case: dict) -> dict[str, str]:
    """{"emacs.csv": rows, "emacs.txt": text} from one Emacs run."""
    name = f"agd-emacs-{uuid.uuid4().hex}"
    start = datetime.date.fromisoformat(case["from"])
    end = datetime.date.fromisoformat(case["to"])
    days = (end - start).days + 1
    environment = {
        "AGD_CASE_DIR": "/case",
        "AGD_FROM": case["from"],
        "AGD_TODAY": case["today"],
        "AGD_DAYS": str(days),
        "AGD_SKIP_SCHEDULED_IF_DONE": str(
            case.get("skip_scheduled_if_done", False)
        ).lower(),
        "AGD_SKIP_DEADLINE_IF_DONE": str(
            case.get("skip_deadline_if_done", False)
        ).lower(),
        "AGD_SKIP_TIMESTAMP_IF_DONE": str(
            case.get("skip_timestamp_if_done", False)
        ).lower(),
        "AGD_LOG": str(case.get("log", False)).lower(),
    }
    if case.get("span"):
        environment["AGD_SPAN"] = case["span"]
    if case.get("sorting_strategy"):
        environment["AGD_SORTING"] = " ".join(case["sorting_strategy"])
    if case.get("match"):
        environment["AGD_MATCH"] = case["match"]
        environment["AGD_MATCH_KIND"] = case.get("match_kind", "tags")
    create = ["create", "--name", name]
    for key, value in environment.items():
        create.extend(["--env", f"{key}={value}"])
    create.extend([IMAGE, "emacs", "-Q", "--batch", "-l", "/oracle.el"])
    try:
        docker(*create)
        docker("cp", str(case_dir) + "/.", f"{name}:/case")
        oracle = pathlib.Path(__file__).resolve().parent.parent / "conformance" / "emacs-agenda-oracle.el"
        docker("cp", str(oracle), f"{name}:/oracle.el")
        raw = docker("start", "-a", name, capture=True)
    finally:
        subprocess.run(
            ["docker", "rm", "-f", name],
            stdout=subprocess.DEVNULL,
            stderr=subprocess.DEVNULL,
        )
    # The oracle prints the CSV rows, a form-feed line, then the text agenda.
    # Row order is the org-agenda sorting strategy under test.
    rows, text = raw.split("\f\n", 1)
    return {"emacs.csv": rows, "emacs.txt": text}


def cases(root: pathlib.Path):
    if (root / "case.toml").is_file():
        yield root
        return
    yield from sorted(path.parent for path in root.glob("*/case.toml"))


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("root", type=pathlib.Path, nargs="?", default="conformance/agenda")
    parser.add_argument("--update", action="store_true")
    arguments = parser.parse_args()
    failed = False
    for case_dir in cases(arguments.root.resolve()):
        with (case_dir / "case.toml").open("rb") as file:
            case = tomllib.load(file)
        for file_name, actual in emacs_outputs(case_dir, case).items():
            expected_path = case_dir / file_name
            if arguments.update:
                expected_path.write_text(actual, encoding="utf-8")
                print(f"updated {expected_path}")
            elif not expected_path.is_file() or expected_path.read_text(encoding="utf-8") != actual:
                print(f"Emacs oracle drift: {case_dir.name}/{file_name}", file=sys.stderr)
                failed = True
    return int(failed)


if __name__ == "__main__":
    raise SystemExit(main())
