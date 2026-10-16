import unittest

from ingest.dedupe import dedupe


class DedupeTest(unittest.TestCase):
    def test_keeps_the_newest_record(self):
        records = [
            {"id": 1, "updated_at": "2026-10-01T10:00:00Z", "amount": 10},
            {"id": 1, "updated_at": "2026-10-02T10:00:00Z", "amount": 12},
        ]
        self.assertEqual(dedupe(records), [records[1]])


if __name__ == "__main__":
    unittest.main()
