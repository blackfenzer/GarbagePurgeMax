SELECT path_key, display_path, logical_bytes, created_unix_seconds,
       created_subsec_nanos, modified_unix_seconds, modified_subsec_nanos, read_only
FROM files
WHERE scan_id = :scan_id
  AND (:search = '' OR instr(lower(display_path), lower(:search)) > 0)
ORDER BY logical_bytes DESC, path_key
LIMIT :limit OFFSET :offset;
