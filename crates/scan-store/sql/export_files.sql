SELECT files.path_key, display_path, files.logical_bytes, created_unix_seconds,
       created_subsec_nanos, modified_unix_seconds, modified_subsec_nanos, read_only,
       duplicate_members.group_id, similar_image_members.group_id, similar_image_members.distance
FROM files
LEFT JOIN duplicate_members USING (scan_id, path_key)
LEFT JOIN similar_image_members USING (scan_id, path_key)
WHERE files.scan_id = :scan_id
ORDER BY files.logical_bytes DESC, files.path_key;
