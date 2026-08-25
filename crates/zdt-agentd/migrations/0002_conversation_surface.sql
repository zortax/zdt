-- The full conversation surface: modes, models, plans, usage, and rich timeline rows.

ALTER TABLE threads ADD COLUMN mode TEXT NOT NULL DEFAULT 'supervised';
ALTER TABLE threads ADD COLUMN model TEXT NOT NULL DEFAULT '';
ALTER TABLE threads ADD COLUMN todos TEXT NOT NULL DEFAULT '[]';
ALTER TABLE threads ADD COLUMN proposed_plan TEXT;
ALTER TABLE threads ADD COLUMN context_tokens INTEGER NOT NULL DEFAULT 0;
ALTER TABLE threads ADD COLUMN context_limit INTEGER NOT NULL DEFAULT 0;
ALTER TABLE threads ADD COLUMN cost_usd REAL NOT NULL DEFAULT 0;

ALTER TABLE messages ADD COLUMN name TEXT NOT NULL DEFAULT '';
ALTER TABLE messages ADD COLUMN tool TEXT NOT NULL DEFAULT '';
ALTER TABLE messages ADD COLUMN status TEXT NOT NULL DEFAULT 'ok';
ALTER TABLE messages ADD COLUMN detail TEXT NOT NULL DEFAULT '';
