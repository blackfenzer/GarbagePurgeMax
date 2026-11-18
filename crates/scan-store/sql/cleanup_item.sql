SELECT id, scan_id, source_key, source_proof, keeper_key, keeper_proof, reason,
       state, receipt_key, receipt_path, message, created_unix_ms,
       available_before, available_after
FROM cleanup_items
WHERE id = ?1
