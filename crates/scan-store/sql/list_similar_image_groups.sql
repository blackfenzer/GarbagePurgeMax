SELECT group_id, file_count, SUM(files.logical_bytes) AS group_bytes
FROM similar_image_groups
JOIN similar_image_members USING (scan_id, group_id)
JOIN files USING (scan_id, path_key)
WHERE similar_image_groups.scan_id = :scan_id
GROUP BY group_id
ORDER BY group_bytes DESC, group_id
LIMIT :limit OFFSET :offset;
