import sqlite3
import unittest
from pathlib import Path


SQL = Path(__file__).resolve().parents[1] / "sql"


class SchemaTests(unittest.TestCase):
    def setUp(self):
        self.db = sqlite3.connect(":memory:")
        self.addCleanup(self.db.close)
        # The app registers the scanner's type names; the schema tests only need the hook.
        self.db.create_function("file_type", 1, lambda path: "Other", deterministic=True)
        schema = "\n".join(
            (SQL / migration).read_text(encoding="utf-8")
            for migration in ("001_initial.sql", "002_exact_duplicates.sql", "003_similar_images.sql")
        )
        self.db.executescript(f"PRAGMA foreign_keys = ON; BEGIN;\n{schema}\nCOMMIT;")

    def add_scan(self):
        return self.db.execute(
            """INSERT INTO scans
               (root_key, display_root, started_unix_ms, status, large_file_threshold)
               VALUES (?, '/sample', 1000, 'running', 1073741824)""",
            (b"/sample",),
        ).lastrowid

    def add_file(self, scan_id, key, display, size):
        self.db.execute(
            """INSERT INTO files (scan_id, path_key, display_path, logical_bytes, read_only)
               VALUES (?, ?, ?, ?, 0)""",
            (scan_id, key, display, size),
        )

    def finish_scan(self, scan_id, status):
        self.db.execute(
            "UPDATE scans SET status = ?, finished_unix_ms = 2000 WHERE id = ?",
            (status, scan_id),
        )

    def list_files(self, scan, search="", limit=10, offset=0):
        query = (SQL / "list_files.sql").read_text(encoding="utf-8")
        query += "ORDER BY 3 DESC, 1\nLIMIT :limit OFFSET :offset"
        return self.db.execute(
            query,
            {
                "scan_id": scan, "search": search, "limit": limit, "offset": offset,
                "file_type": "", "modified_before": None,
            },
        ).fetchall()

    def test_history_includes_cancelled_scans_and_excludes_in_progress_scans(self):
        self.assertEqual(self.db.execute("PRAGMA user_version").fetchone(), (3,))
        completed, cancelled, _ = (self.add_scan() for _ in range(3))
        self.finish_scan(completed, "completed")
        self.finish_scan(cancelled, "cancelled")

        query = (SQL / "list_scans.sql").read_text(encoding="utf-8")
        history = self.db.execute(query, {"limit": 10, "offset": 0}).fetchall()
        self.assertEqual([(row[0], row[4]) for row in history], [
            (cancelled, "cancelled"), (completed, "completed"),
        ])

    def test_search_is_literal_scoped_and_paginated_in_size_order(self):
        scan, other_scan = self.add_scan(), self.add_scan()
        self.add_file(scan, b"b", "100%_EXPORT.zip", 900)
        self.add_file(scan, b"a", "50%_Export.zip", 900)
        self.add_file(scan, b"c", "big-export.zip", 1200)
        self.add_file(other_scan, b"d", "other%_export.zip", 2000)

        self.assertEqual([row[0] for row in self.list_files(scan)], [b"c", b"a", b"b"])
        self.assertEqual([row[0] for row in self.list_files(scan, "%_eXpOrT")], [b"a", b"b"])
        self.assertEqual(self.list_files(scan, "%_EXPORT", limit=1, offset=1)[0][0], b"b")

    def test_distinct_native_paths_can_share_a_display_name(self):
        scan = self.add_scan()
        self.add_file(scan, b"file\xff", "file\ufffd", 60)
        self.add_file(scan, b"file\xfe", "file\ufffd", 60)
        self.assertEqual(len(self.list_files(scan)), 2)
        with self.assertRaises(sqlite3.IntegrityError):
            self.add_file(scan, b"file\xff", "same native path", 60)

    def test_rollback_removes_scan_metadata_and_foreign_keys_prevent_orphans(self):
        scan = self.add_scan()
        self.add_file(scan, b"a", "notes.txt", 120)
        self.db.rollback()

        self.assertEqual(self.db.execute("SELECT COUNT(*) FROM scans").fetchone(), (0,))
        self.assertEqual(self.list_files(scan), [])
        with self.assertRaises(sqlite3.IntegrityError):
            self.add_file(scan, b"a", "notes.txt", 120)

    def test_similar_members_need_indexed_files_and_a_bounded_distance(self):
        scan = self.add_scan()
        self.add_file(scan, b"a.jpg", "a.jpg", 10)
        self.db.execute(
            "INSERT INTO similar_image_groups (scan_id, group_id, file_count) VALUES (?, 1, 2)",
            (scan,),
        )
        insert = """INSERT INTO similar_image_members
                    (scan_id, path_key, group_id, position, difference_hash, distance)
                    VALUES (?, ?, 1, ?, -2, ?)"""
        self.db.execute(insert, (scan, b"a.jpg", 0, 0))
        with self.assertRaises(sqlite3.IntegrityError):
            self.db.execute(insert, (scan, b"missing.jpg", 1, 3))
        self.add_file(scan, b"b.jpg", "b.jpg", 10)
        with self.assertRaises(sqlite3.IntegrityError):
            self.db.execute(insert, (scan, b"b.jpg", 1, 65))


if __name__ == "__main__":
    unittest.main()
