SELECT EXISTS(SELECT 1 FROM directories WHERE scan_id = ?1 AND path_key = ?2 AND empty = 1)
