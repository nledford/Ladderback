-- The persisted names say machine, not agent (§16, decision 163). Every
-- table, column and index that named a machine `agent` is renamed; the rows
-- are kept as they are.
ALTER TABLE agents RENAME TO machines;
ALTER TABLE agent_leases RENAME TO machine_leases;
ALTER TABLE agent_epoch_floors RENAME TO machine_epoch_floors;
ALTER TABLE agent_tombstones RENAME TO machine_tombstones;
ALTER TABLE agent_maintenance_paused RENAME TO machine_maintenance_paused;

ALTER TABLE operations RENAME COLUMN agent_id TO machine_id;
ALTER TABLE sync_watermarks RENAME COLUMN agent_id TO machine_id;
ALTER TABLE sync_reports RENAME COLUMN agent_id TO machine_id;
ALTER TABLE sync_report_tallies RENAME COLUMN agent_id TO machine_id;
ALTER TABLE ownership_reports RENAME COLUMN agent_id TO machine_id;
ALTER TABLE sync_plans RENAME COLUMN agent_id TO machine_id;
ALTER TABLE sync_plan_snapshots RENAME COLUMN agent_id TO machine_id;
ALTER TABLE sync_progress RENAME COLUMN agent_id TO machine_id;

ALTER TABLE pairing_tokens RENAME COLUMN used_agent_id TO used_machine_id;
ALTER TABLE pairing_tokens RENAME COLUMN used_agent_key TO used_machine_key;

DROP INDEX sync_reports_by_agent;
CREATE INDEX sync_reports_by_machine ON sync_reports (machine_id, id);

-- A failed hook's level is stored with its error; the machine level's code
-- is now `machine`.
UPDATE operations
SET error_args = replace(
    error_args,
    '["level",{"type":"text","value":"agent"}]',
    '["level",{"type":"text","value":"machine"}]'
)
WHERE error_code = 'hook-failed';
