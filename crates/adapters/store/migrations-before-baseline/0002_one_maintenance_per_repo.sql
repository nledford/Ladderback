-- At most one running maintenance operation per repository (invariant I7,
-- technical design §9). `kind` is the operation's kind: forget, prune and check
-- are the maintenance ones (the domain has no forget kind yet; the index names
-- it so a later kind needs no migration).
--
-- An earlier build did not stop two of them running on one repository, so the
-- index can't be created until the log obeys the rule. Of the running
-- maintenance rows on one repository the newest by ID stays running; each
-- older one is demoted to failed with the engine-exited code and no arguments
-- (the stored spelling of "no arguments" is '[]'), as a run that died without
-- an exit status. Its finished_at stays NULL: when it really ended is unknown,
-- and no reader needs a finish time of a finished operation.
UPDATE operations
SET status = 'failed', error_code = 'engine-exited', error_args = '[]', error_detail = NULL
WHERE status = 'running' AND kind IN ('forget', 'prune', 'check') AND repo_uuid IS NOT NULL
  AND EXISTS (SELECT 1 FROM operations AS newer
              WHERE newer.repo_uuid = operations.repo_uuid AND newer.status = 'running'
                AND newer.kind IN ('forget', 'prune', 'check') AND newer.id > operations.id);

-- A partial unique index: a repository appears at most once among the running
-- maintenance rows. Operations with no repository are not covered.
CREATE UNIQUE INDEX one_maintenance_per_repo ON operations(repo_uuid)
  WHERE status = 'running' AND kind IN ('forget', 'prune', 'check');
