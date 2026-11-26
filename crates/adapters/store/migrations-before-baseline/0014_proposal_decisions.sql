-- What a person decided about a proposal (technical design §9; decision
-- 89): when, why a rejection was made and where an approval came from, and
-- what its apply kept. Additions only, no data changes and no table rebuilt:
--
-- decided_at is when a person rejected or approved it, Unix MILLISECONDS,
-- UTC. reasons is a rejection's reasons, one per change in order, as a JSON
-- array of strings. approved_from is the address of the browser a person
-- approved it from, as text. applied_as names the configuration history's
-- version its apply kept. Each is NULL until its step happens.
ALTER TABLE proposals ADD COLUMN decided_at INTEGER;
ALTER TABLE proposals ADD COLUMN reasons TEXT;
ALTER TABLE proposals ADD COLUMN approved_from TEXT;
ALTER TABLE proposals ADD COLUMN applied_as TEXT;
