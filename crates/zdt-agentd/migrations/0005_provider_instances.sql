-- Which configured provider instance drives each thread. The harness itself is the provider
-- column from the first schema.
ALTER TABLE threads ADD COLUMN instance TEXT NOT NULL DEFAULT '';
