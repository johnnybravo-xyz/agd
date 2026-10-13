#!/usr/bin/env python3
import json
import pathlib
import subprocess
import uuid

IMAGE = "silex/emacs:30.2-alpine@sha256:2dd43b22af2e2592753ad92c3cb78fc580ce5981fc4d8ad1edee60ec92241ae5"
ROOT = pathlib.Path(__file__).resolve().parent.parent


def check(case: pathlib.Path) -> None:
    name = f"agd-daemon-{uuid.uuid4().hex}"
    script = (
        "emacs --daemon=agd -Q -l /fixture/init.el >/tmp/emacs.log 2>&1 && "
        "emacsclient --socket-name=agd --eval "
        "'(progn (load \"/probe.el\" nil nil t) (agd-import-probe))'; "
        "emacsclient --socket-name=agd --eval '(kill-emacs)' >/dev/null 2>&1 || true"
    )
    try:
        subprocess.run(
            [
                "docker", "create", "--name", name,
                "--env", "AGD_FIXTURE_DIR=/fixture",
                "--env", "AGD_IMPORT_SOURCE=daemon",
                IMAGE, "sh", "-c", script,
            ],
            check=True,
            stdout=subprocess.DEVNULL,
        )
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
    encoded = next((line for line in output.splitlines() if line.startswith('"{')), None)
    if encoded is None:
        raise RuntimeError(f"daemon probe result missing for {case.name}: {output}")
    actual = json.loads(json.loads(encoded))
    expected = json.loads((case / "resolved.json").read_text(encoding="utf-8"))
    actual["source"] = expected["source"]
    if actual != expected:
        raise RuntimeError(f"daemon and batch profiles differ for {case.name}")


def main() -> None:
    root = ROOT / "conformance/import/emacs"
    check(root / "vanilla")
    check(root / "doom")
    print("Emacs daemon and batch probes agree")


if __name__ == "__main__":
    main()
