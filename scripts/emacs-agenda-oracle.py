#!/usr/bin/env python3
import argparse
import csv
import datetime
import io
import pathlib
import subprocess
import sys
import tomllib
import uuid

IMAGE = "silex/emacs:30.2-alpine@sha256:2dd43b22af2e2592753ad92c3cb78fc580ce5981fc4d8ad1edee60ec92241ae5"
KINDS = {
    "scheduled",
    "past-scheduled",
    "deadline",
    "upcoming-deadline",
    "timestamp",
    "block",
    "closed",
    "todo",
}


def docker(*args: str, capture: bool = False) -> str:
    result = subprocess.run(
        ["docker", *args],
        check=True,
        text=True,
        stdout=subprocess.PIPE if capture else subprocess.DEVNULL,
    )
    return result.stdout if capture else ""


def emacs_rows(case_dir: pathlib.Path, case: dict) -> str:
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
    }
    if case.get("sorting_strategy"):
        environment["AGD_SORTING"] = " ".join(case["sorting_strategy"])
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

    rows = []
    for row in csv.reader(io.StringIO(raw)):
        if len(row) < 9 or row[2] not in KINDS:
            continue
        if len(row) > 10:
            row[5] = row[10]
        row[6] = row[6].rstrip(".")
        if row[2] == "past-scheduled" and row[7] == "Scheduled:":
            row[2] = "scheduled"
        rows.append(row[:9])
    # Keep Emacs row order: it is the org-agenda sorting strategy under test.
    output = io.StringIO()
    writer = csv.writer(output, lineterminator="\n")
    writer.writerows(rows)
    return output.getvalue()


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
        actual = emacs_rows(case_dir, case)
        expected_path = case_dir / "emacs.csv"
        if arguments.update:
            expected_path.write_text(actual, encoding="utf-8")
            print(f"updated {expected_path}")
        elif not expected_path.is_file() or expected_path.read_text(encoding="utf-8") != actual:
            print(f"Emacs oracle drift: {case_dir.name}", file=sys.stderr)
            failed = True
    return int(failed)


if __name__ == "__main__":
    raise SystemExit(main())
