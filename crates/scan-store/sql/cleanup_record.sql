SELECT path_key, display_path, logical_bytes, created_unix_seconds,
       created_subsec_nanos, modified_unix_seconds, modified_subsec_nanos, read_only
FROM files WHERE scan_id = ?1 AND path_key = ?2
