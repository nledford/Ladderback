-- The alias of the repository a maintenance operation ran on, as this agent
-- named the repository when the run started (technical design §16, decision
-- 52): the Operations page names a maintenance row by it, and a later rename
-- of the alias leaves the log as it was. NULL for every other operation and
-- for every row logged before this build, which the page then names by what
-- ran alone. Adding a nullable column rewrites no row.
ALTER TABLE operations ADD COLUMN repo_alias TEXT;
