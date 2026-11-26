-- The entry of a snapshot a restore or a download read (technical design §9,
-- #847): its path from the snapshot's top as a person reads it, `/` first,
-- with every secret of the repository masked, every character that would
-- break or reorder a line made visible, and a path longer than 1 KiB cut to
-- its end after "…". NULL for every other operation and for every row logged
-- before this build. Adding a nullable column rewrites no row; like decision
-- 52's column, the change is one-way.
ALTER TABLE operations ADD COLUMN entry TEXT;
