SELECT group_id, file_count, SUM(logical_bytes) AS group_bytes
FROM duplicate_groups
JOIN duplicate_members USING (scan_id, group_id)
WHERE duplicate_groups.scan_id = :scan_id
GROUP BY group_id
ORDER BY group_bytes DESC, group_id
LIMIT :limit OFFSET :offset;
