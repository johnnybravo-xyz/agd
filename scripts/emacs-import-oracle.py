#!/usr/bin/env python3
import argparse
import json
import pathlib
import subprocess
import uuid

IMAGE = "silex/emacs:29.4-alpine@sha256:2aef086ce79273d2fd0af2519a4e81b8b3ae5e225bf62c0e994a10f487d6f7d7"
ROOT = pathlib.Path(__file__).resolve().parent.parent


def resolved(case: pathlib.Path) -> dict:
    name = f"agendum-import-{uuid.uuid4().hex}"
    command = [
        "docker",
        "create",
        "--name",
        name,
        "--env",
        "AGENDUM_FIXTURE_DIR=/fixture",
        "--env",
        "AGENDUM_IMPORT_SOURCE=batch-profile",
        "--env",
        "AGENDUM_PROBE_STDOUT=1",
        IMAGE,
        "emacs",
        "-Q",
        "--batch",
        "-l",
        "/fixture/init.el",
        "-l",
        "/probe.el",
    ]
    try:
        subprocess.run(command, check=True, stdout=subprocess.DEVNULL)
        subprocess.run(
            ["docker", "cp", str(case) + "/.", f"{name}:/fixture"],
            check=True,
            stdout=subprocess.DEVNULL,
        )
        subprocess.run(
            ["docker", "cp", str(ROOT / "agd/probes/emacs.el"), f"{name}:/probe.el"],
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
    marker = "AGENDUM_JSON:"
    payload = next((line[len(marker):] for line in output.splitlines() if line.startswith(marker)), None)
    if payload is None:
        raise RuntimeError(f"probe marker missing for {case}")
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
