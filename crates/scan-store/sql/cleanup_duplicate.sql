SELECT m.group_id, g.hash FROM duplicate_members m JOIN duplicate_groups g USING(scan_id, group_id) WHERE m.scan_id = ?1 AND m.path_key = ?2
