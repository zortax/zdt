-- How hard a thread's agent reasons, in the provider's own words. Empty means its default.
ALTER TABLE threads ADD COLUMN effort TEXT NOT NULL DEFAULT '';
