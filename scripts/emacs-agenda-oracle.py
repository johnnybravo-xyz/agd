#!/usr/bin/env python3
"""Writes or checks each conformance/agenda case's emacs.csv and emacs.txt:
pinned Emacs's org-batch-agenda-csv rows and org-batch-agenda text, byte for
byte. agd's --csv and text agenda must print the same."""
import argparse
import datetime
import pathlib
import sys
import tomllib

from oracle import ROOT, check, run_emacs


def emacs_outputs(case_dir: pathlib.Path, case: dict) -> dict[str, str]:
    """{"emacs.csv": rows, "emacs.txt": text} from one Emacs run."""
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
    raw = run_emacs(
        "/oracle.el",
        environment,
        [(f"{case_dir}/.", "/case"), (ROOT / "conformance/emacs-agenda-oracle.el", "/oracle.el")],
    )
    # The oracle prints the CSV rows, a form-feed line, then the text agenda.
    # Row order is the org-agenda sorting strategy under test.
    rows, text = raw.split("\f\n", 1)
    return {"emacs.csv": rows, "emacs.txt": text}


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--update", action="store_true")
    arguments = parser.parse_args()
    failed = False
    for case_dir in sorted(path.parent for path in (ROOT / "conformance/agenda").glob("*/case.toml")):
        with (case_dir / "case.toml").open("rb") as file:
            case = tomllib.load(file)
        for file_name, actual in emacs_outputs(case_dir, case).items():
            if not check(case_dir / file_name, actual, arguments.update):
                print(f"Emacs oracle drift: {case_dir.name}/{file_name}", file=sys.stderr)
                failed = True
    return int(failed)


if __name__ == "__main__":
    raise SystemExit(main())
