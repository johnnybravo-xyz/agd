"""Backfill a date range from the vendor API, one page at a time."""
import json
import urllib.request

from ingest.dedupe import dedupe

PAGE_URL = "https://vendor.example.com/v2/records?day={day}&page={page}"


def fetch_page(day, page):
    with urllib.request.urlopen(PAGE_URL.format(day=day, page=page)) as response:
        return json.load(response)


def backfill(days):
    records = []
    for day in days:
        page = 1
        while True:
            body = fetch_page(day, page)
            records.extend(body["records"])
            if not body["next"]:
                break
            page += 1
    return dedupe(records)
