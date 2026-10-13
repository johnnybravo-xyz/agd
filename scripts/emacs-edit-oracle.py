#!/usr/bin/env python3
"""Run each conformance/edit/<case> through pinned Emacs and compare or
update its expected.org."""
import argparse
import pathlib
import subprocess
import tomllib
import uuid

IMAGE = "silex/emacs:30.2-alpine@sha256:2dd43b22af2e2592753ad92c3cb78fc580ce5981fc4d8ad1edee60ec92241ae5"
ROOT = pathlib.Path(__file__).resolve().parent.parent
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
    name = f"agd-edit-{uuid.uuid4().hex}"
    command = ["docker", "create", "--user", "0", "--name", name]
    for key, value in environment.items():
        command.extend(["--env", f"{key}={value}"])
    command.extend([IMAGE, "sh", "-c", "exec emacs -Q --batch -l /oracle.el"])
    try:
        subprocess.run(command, check=True, stdout=subprocess.DEVNULL)
        subprocess.run(
            ["docker", "cp", str(case_dir / "input.org"), f"{name}:/root/input.org"],
            check=True,
            stdout=subprocess.DEVNULL,
        )
        subprocess.run(
            ["docker", "cp", str(ROOT / "conformance/emacs-edit-oracle.el"), f"{name}:/oracle.el"],
            check=True,
            stdout=subprocess.DEVNULL,
        )
        subprocess.run(["docker", "start", "-a", name], check=True, stdout=subprocess.DEVNULL)
        subprocess.run(
            ["docker", "cp", f"{name}:/root/input.org", str(destination)],
            check=True,
            stdout=subprocess.DEVNULL,
        )
    finally:
        subprocess.run(["docker", "rm", "-f", name], stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)


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
