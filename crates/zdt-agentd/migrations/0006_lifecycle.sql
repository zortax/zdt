-- Lifecycle overlays and drafts: four independent axes on a thread, and the prompt not sent yet.
ALTER TABLE threads ADD COLUMN pinned REAL NOT NULL DEFAULT 0;
ALTER TABLE threads ADD COLUMN snoozed_until INTEGER NOT NULL DEFAULT 0;
ALTER TABLE threads ADD COLUMN settled INTEGER NOT NULL DEFAULT 0;
ALTER TABLE threads ADD COLUMN archived INTEGER NOT NULL DEFAULT 0;
ALTER TABLE threads ADD COLUMN unread INTEGER NOT NULL DEFAULT 0;
ALTER TABLE threads ADD COLUMN draft TEXT NOT NULL DEFAULT '';
