-- Adoption is a maintenance operation (technical design §7.3, step 3): it
-- holds the repository's one running maintenance slot like forget, prune and
-- check. The index migration 0002 created names the kinds, so it is dropped
-- and recreated with 'adopt' among them. No data changes: no running adopt
-- row can exist before this build.
DROP INDEX IF EXISTS one_maintenance_per_repo;

CREATE UNIQUE INDEX one_maintenance_per_repo ON operations(repo_uuid)
  WHERE status = 'running' AND kind IN ('forget', 'prune', 'check', 'adopt');
