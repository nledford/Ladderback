-- The machine zone each agent last reported (technical design §7.1, §7.2;
-- #114): the IANA name of the time zone the machine runs in, as the hub's
-- copy of the zone database spells it, or NULL when the agent reported none
-- or one the hub's database doesn't know. The hub shows the machine's times
-- in it and never schedules by it. One addition, no data changes: the agents
-- already registered hold NULL until their next connection.
ALTER TABLE agents ADD COLUMN zone TEXT;
