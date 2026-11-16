SELECT files.path_key, display_path, logical_bytes, created_unix_seconds,
       created_subsec_nanos, modified_unix_seconds, modified_subsec_nanos, read_only,
       difference_hash, distance
FROM similar_image_members
JOIN files USING (scan_id, path_key)
WHERE similar_image_members.scan_id = :scan_id AND group_id = :group_id
ORDER BY position
LIMIT :limit;
