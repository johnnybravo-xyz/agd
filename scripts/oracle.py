"""What the editor oracles share: the pinned Emacs image, one Emacs run in a
throwaway container, a reader's AGD_JSON payload, and writing or checking an
expected file."""
import json
import pathlib
import subprocess
import uuid

IMAGE = "silex/emacs:30.2-alpine@sha256:2dd43b22af2e2592753ad92c3cb78fc580ce5981fc4d8ad1edee60ec92241ae5"
ROOT = pathlib.Path(__file__).resolve().parent.parent


def run_emacs(script, environment, copies, copy_out=None):
    """Runs `emacs -Q --batch -l SCRIPT` in a fresh container of IMAGE with
    `environment`, after copying each (local path, container path) in
    `copies` into it, and returns its stdout. `copy_out`, a (container path,
    local path), is copied back out after the run."""
    name = f"agd-oracle-{uuid.uuid4().hex}"
    create = ["docker", "create", "--name", name]
    for key, value in environment.items():
        create.extend(["--env", f"{key}={value}"])
    create.extend([IMAGE, "emacs", "-Q", "--batch", "-l", script])
    try:
        subprocess.run(create, check=True, stdout=subprocess.DEVNULL)
        for source, destination in copies:
            subprocess.run(
                ["docker", "cp", str(source), f"{name}:{destination}"],
                check=True,
                stdout=subprocess.DEVNULL,
            )
        output = subprocess.run(
            ["docker", "start", "-a", name], check=True, text=True, stdout=subprocess.PIPE
        ).stdout
        if copy_out:
            subprocess.run(
                ["docker", "cp", f"{name}:{copy_out[0]}", str(copy_out[1])],
                check=True,
                stdout=subprocess.DEVNULL,
            )
    finally:
        subprocess.run(["docker", "rm", "-f", name], stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
    return output


def agd_json(output):
    """The payload a reader prints after `AGD_JSON:`, parsed; None when it
    printed none."""
    marker = "AGD_JSON:"
    payload = next((line[len(marker):] for line in output.splitlines() if line.startswith(marker)), None)
    return None if payload is None else json.loads(payload)


def check(path, actual, update):
    """With `update`, writes `actual` to `path` and says so; otherwise
    whether `path` holds exactly `actual`."""
    if update:
        path.write_text(actual, encoding="utf-8")
        print(f"updated {path}")
        return True
    return path.is_file() and path.read_text(encoding="utf-8") == actual
