UPDATE cleanup_items SET state = 'moving', updated_unix_ms = ?2 WHERE id = ?1 AND state = 'queued'
