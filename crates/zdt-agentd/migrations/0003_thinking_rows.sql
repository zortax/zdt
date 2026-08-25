-- Thinking rows are written down like tool rows, and carry how long the thought took.
ALTER TABLE messages ADD COLUMN elapsed_ms INTEGER NOT NULL DEFAULT 0;
