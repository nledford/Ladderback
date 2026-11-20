-- API tokens carry permissions in place of scopes (technical design §10.1,
-- decision 127). permissions is the rows a token has, in the order a person
-- sees them, as row:level pairs joined by commas, as
-- 'backups:read,backup-runs:start-and-cancel'. Every kept token keeps exactly
-- what its scopes opened and no more: read becomes backups, snapshot contents
-- and the daemon's log, each at read; run, backup runs alone (it opened no
-- route that reads); preview and propose, their rows.
ALTER TABLE api_tokens ADD COLUMN permissions TEXT NOT NULL DEFAULT '';

UPDATE api_tokens SET permissions = rtrim(
  CASE WHEN instr(',' || scopes || ',', ',read,') > 0 THEN 'backups:read,' ELSE '' END
  || CASE WHEN instr(',' || scopes || ',', ',run,') > 0 THEN 'backup-runs:start-and-cancel,' ELSE '' END
  || CASE WHEN instr(',' || scopes || ',', ',preview,') > 0 THEN 'previews:preview,' ELSE '' END
  || CASE WHEN instr(',' || scopes || ',', ',propose,') > 0 THEN 'configuration:propose,' ELSE '' END
  || CASE WHEN instr(',' || scopes || ',', ',read,') > 0 THEN 'snapshot-contents:read,' ELSE '' END
  || CASE WHEN instr(',' || scopes || ',', ',read,') > 0 THEN 'daemon-log:read,' ELSE '' END,
  ','
);

ALTER TABLE api_tokens DROP COLUMN scopes;
