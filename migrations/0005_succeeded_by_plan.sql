-- Whether any operation of a plan succeeded (decision 53): the Repositories
-- page offers to create a saved repository whose latest operation found no
-- repository only when none of its plans' operations ever succeeded. A partial
-- index of the successes alone answers that with one probe, however many
-- operations of the plan failed. Additive: an index, no data changes, and the
-- table isn't rebuilt.
CREATE INDEX ops_succeeded_by_plan ON operations(plan_id)
  WHERE status IN ('succeeded', 'succeeded_with_warnings');
