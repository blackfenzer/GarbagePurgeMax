SELECT id, root_key, started_unix_ms, finished_unix_ms, status,
       files, directories, logical_bytes, large_files, large_file_threshold,
       skipped, errors, first_error
FROM scans
WHERE status <> 'running'
ORDER BY started_unix_ms DESC, id DESC
LIMIT :limit OFFSET :offset;
