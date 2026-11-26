-- Which location of a repository reached at several an operation used, and
-- every location as the run's choice saw it (technical design §9, decision
-- 80): `location_no` is the position, from 1, of the one used, and
-- `locations_json` lists each location in order with its address as the
-- entry wrote it, credentials masked, and its outcome. Both are NULL for a
-- repository of one location, for every operation that opens none, and for
-- every row logged before this build. Adding nullable columns rewrites no
-- row; like decision 52's column, the change is one-way.
ALTER TABLE operations ADD COLUMN location_no INTEGER;
ALTER TABLE operations ADD COLUMN locations_json TEXT;
