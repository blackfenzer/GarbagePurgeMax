SELECT duplicate_members.path_key, display_path, duplicate_members.logical_bytes, created_unix_seconds,
       created_subsec_nanos, modified_unix_seconds, modified_subsec_nanos, read_only
FROM duplicate_members
JOIN files USING (scan_id, path_key)
WHERE duplicate_members.scan_id = :scan_id
  AND (:search = '' OR instr(lower(display_path), lower(:search)) > 0)
ORDER BY duplicate_members.logical_bytes DESC, duplicate_members.path_key
LIMIT :limit OFFSET :offset;
