UPDATE cleanup_items SET receipt_key = ?2, receipt_path = ?3 WHERE id = ?1 AND state = 'moving'
