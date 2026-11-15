SELECT path_key, display_path, logical_bytes, created_unix_seconds,
       created_subsec_nanos, modified_unix_seconds, modified_subsec_nanos, read_only
FROM files INDEXED BY files_by_size
WHERE scan_id = ?1
  AND logical_bytes = (
      SELECT logical_bytes
      FROM files
      WHERE scan_id = ?1 AND logical_bytes > ?2
      GROUP BY logical_bytes
      HAVING COUNT(*) > 1
      ORDER BY logical_bytes
      LIMIT 1
  )
ORDER BY path_key;
