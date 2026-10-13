#!/usr/bin/env python3
import json
import pathlib
import subprocess
import uuid

IMAGE = "silex/emacs:29.4-alpine@sha256:2aef086ce79273d2fd0af2519a4e81b8b3ae5e225bf62c0e994a10f487d6f7d7"
ROOT = pathlib.Path(__file__).resolve().parent.parent


def check(case: pathlib.Path) -> None:
    name = f"agendum-daemon-{uuid.uuid4().hex}"
    script = (
        "emacs --daemon=agendum -Q -l /fixture/init.el >/tmp/emacs.log 2>&1 && "
        "emacsclient --socket-name=agendum --eval "
        "'(progn (load \"/probe.el\" nil nil t) (agendum-import-probe))'; "
        "emacsclient --socket-name=agendum --eval '(kill-emacs)' >/dev/null 2>&1 || true"
    )
    try:
        subprocess.run(
            [
                "docker", "create", "--name", name,
                "--env", "AGENDUM_FIXTURE_DIR=/fixture",
                "--env", "AGENDUM_IMPORT_SOURCE=daemon",
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
