SELECT files.path_key, display_path, files.logical_bytes, created_unix_seconds,
       created_subsec_nanos, modified_unix_seconds, modified_subsec_nanos, read_only
FROM duplicate_members
JOIN files USING (scan_id, path_key)
WHERE duplicate_members.scan_id = :scan_id AND group_id = :group_id
ORDER BY files.path_key
LIMIT :limit;
