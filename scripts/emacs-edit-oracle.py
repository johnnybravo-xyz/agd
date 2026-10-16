#!/usr/bin/env python3
"""Run each conformance/edit/<case> through pinned Emacs and compare or
update its expected.org."""
import argparse
import pathlib
import tomllib

from oracle import ROOT, run_emacs

LOG_SETTINGS = ("log_done", "log_repeat", "log_reschedule", "log_redeadline")


def run(case_dir: pathlib.Path, destination: pathlib.Path) -> None:
    with (case_dir / "case.toml").open("rb") as file:
        case = tomllib.load(file)
    environment = {
        "AGD_EDIT_OPERATION": case["operation"],
        "AGD_EDIT_VALUE": case["value"],
        "AGD_NOW": case["now"],
        "AGD_LOG_INTO_DRAWER": "t" if case.get("log_into_drawer") else "nil",
    }
    for setting in LOG_SETTINGS:
        environment["AGD_" + setting.upper()] = case.get(setting, "nil")
    run_emacs(
        "/oracle.el",
        environment,
        [(case_dir / "input.org", "/root/input.org"), (ROOT / "conformance/emacs-edit-oracle.el", "/oracle.el")],
        copy_out=("/root/input.org", destination),
    )


def main() -> None:
    parser = argparse.ArgumentParser()
    parser.add_argument("--update", action="store_true")
    arguments = parser.parse_args()
    for case_dir in sorted(path.parent for path in (ROOT / "conformance/edit").glob("*/case.toml")):
        destination = case_dir / "expected.org"
        temporary = destination if arguments.update else case_dir / ".actual.org"
        run(case_dir, temporary)
        if arguments.update:
            print(f"updated {destination}")
        elif temporary.read_bytes() != destination.read_bytes():
            raise SystemExit(f"Emacs edit oracle drift: {case_dir.name}")
        else:
            temporary.unlink()


if __name__ == "__main__":
    main()
