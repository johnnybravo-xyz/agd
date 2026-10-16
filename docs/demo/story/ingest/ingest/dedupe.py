"""Collapse vendor records that share an id, keeping the newest."""


def dedupe(records):
    latest = {}
    for record in records:
        current = latest.get(record["id"])
        if current is None or record["updated_at"] > current["updated_at"]:
            latest[record["id"]] = record
    return list(latest.values())
