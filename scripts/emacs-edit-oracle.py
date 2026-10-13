#!/usr/bin/env python3
import argparse
import pathlib
import subprocess
import uuid

IMAGE = "silex/emacs:30.2-alpine@sha256:2dd43b22af2e2592753ad92c3cb78fc580ce5981fc4d8ad1edee60ec92241ae5"
OPERATIONS = ("state", "schedule", "deadline", "priority")
ROOT = pathlib.Path(__file__).resolve().parent.parent


def run(operation: str, destination: pathlib.Path) -> None:
    name = f"agd-edit-{uuid.uuid4().hex}"
    command = [
        "docker",
        "create",
        "--user",
        "0",
        "--name",
        name,
        "--env",
        f"AGD_EDIT_OPERATION={operation}",
        IMAGE,
        "sh",
        "-c",
        "exec emacs -Q --batch -l /oracle.el",
    ]
    try:
        subprocess.run(command, check=True, stdout=subprocess.DEVNULL)
        subprocess.run(
            ["docker", "cp", str(ROOT / "conformance/edit/base.org"), f"{name}:/root/input.org"],
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
        subprocess.run(
            ["docker", "rm", "-f", name],
            stdout=subprocess.DEVNULL,
            stderr=subprocess.DEVNULL,
        )


def main() -> None:
    parser = argparse.ArgumentParser()
    parser.add_argument("--update", action="store_true")
    arguments = parser.parse_args()
    expected = ROOT / "conformance/edit/expected"
    expected.mkdir(parents=True, exist_ok=True)
    for operation in OPERATIONS:
        destination = expected / f"{operation}.org"
        temporary = destination if arguments.update else expected / f".{operation}.actual.org"
        run(operation, temporary)
        if arguments.update:
            print(f"updated {destination}")
        else:
            if temporary.read_bytes() != destination.read_bytes():
                raise SystemExit(f"Emacs edit oracle drift: {operation}")
            temporary.unlink()


if __name__ == "__main__":
    main()
