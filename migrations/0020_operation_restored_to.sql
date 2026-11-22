-- Where a restore wrote what it restored (technical design §9, #813): ''
-- when it restored over the entry's original path, else the path of the new
-- folder it restored into, every character that would break or reorder a
-- line made visible and a path longer than 1 KiB cut to its end after "…".
-- NULL for every other operation and for every restore logged before this
-- build. Adding a nullable column rewrites no row; like decision 52's
-- column, the change is one-way.
ALTER TABLE operations ADD COLUMN restored_to TEXT;
