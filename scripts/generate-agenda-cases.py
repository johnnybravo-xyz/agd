#!/usr/bin/env python3
import datetime
import pathlib
import random

ROOT = pathlib.Path(__file__).resolve().parent.parent / "conformance" / "agenda" / "generated"
TODAY = datetime.date(2026, 10, 13)


def stamp(day: datetime.date, suffix: str = "") -> str:
    value = day.strftime("%Y-%m-%d %a")
    return f"<{value}{suffix}>"


def main() -> None:
    random_source = random.Random(0xA6D)
    lines = ["#+TODO: TODO NEXT WAIT | DONE CANCELLED", "#+CATEGORY: generated"]
    states = ["TODO", "NEXT", "WAIT", "DONE"]
    kinds = ["scheduled", "deadline", "both", "timestamp", "range"]
    repeaters = ["", " +2d", " ++3d", " .+1w"]
    for number in range(64):
        state = random_source.choice(states)
        priority = random_source.choice(["", " [#A]", " [#B]", " [#C]"])
        tag = random_source.choice(["work", "home", "errand", "project"])
        lines.append(f"* {state}{priority} Generated {number:02d} :{tag}:")
        kind = random_source.choice(kinds)
        first = TODAY + datetime.timedelta(days=random_source.randint(0, 8))
        repeater = random_source.choice(repeaters)
        if repeater and first < TODAY:
            first = TODAY + datetime.timedelta(days=random_source.randint(0, 4))
        if kind == "scheduled":
            lines.append(f"SCHEDULED: {stamp(first, repeater)}")
        if kind == "deadline":
            warning = random_source.choice(["", " -2d", " -5d"])
            lines.append(f"DEADLINE: {stamp(first, warning)}")
        if kind == "both":
            warning = random_source.choice(["", " -2d", " -5d"])
            lines.append(
                f"SCHEDULED: {stamp(first, repeater)} DEADLINE: {stamp(first, warning)}"
            )
        if kind == "timestamp":
            lines.append(stamp(first, repeater))
        if kind == "range":
            end = first + datetime.timedelta(days=random_source.randint(1, 3))
            lines.append(f"{stamp(first)}--{stamp(end)}")
    ROOT.mkdir(parents=True, exist_ok=True)
    (ROOT / "case.toml").write_text(
        'today = "2026-10-13"\nfrom = "2026-10-10"\nto = "2026-10-21"\n',
        encoding="utf-8",
    )
    (ROOT / "tasks.org").write_text("\n".join(lines) + "\n", encoding="utf-8")


if __name__ == "__main__":
    main()
