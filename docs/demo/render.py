#!/usr/bin/env python3
"""Renders docs/demo/org into a directory with dates relative to today.

@D+N@, @D-N@ and @D+N HH:MM@ become org dates with weekday names; any
suffix after the time (a range end, repeater or warning) is kept.
"""
import datetime
import pathlib
import re
import sys

source = pathlib.Path(__file__).resolve().parent / "org"
target = pathlib.Path(sys.argv[1])
today = datetime.date.today()


def date(match: re.Match) -> str:
    day = today + datetime.timedelta(days=int(match.group(1)))
    rest = match.group(2) or ""
    return f"{day:%Y-%m-%d %a}{rest}"


target.mkdir(parents=True, exist_ok=True)
for path in source.glob("*.org"):
    text = path.read_text()
    birthday = today + datetime.timedelta(days=2)
    text = text.replace("@BIRTHDAY@", f"{birthday.replace(year=birthday.year - 30):%Y-%m-%d %a}")
    text = re.sub(r"@D([+-]\d+)((?: [^@]*)?)@", date, text)
    (target / path.name).write_text(text)
