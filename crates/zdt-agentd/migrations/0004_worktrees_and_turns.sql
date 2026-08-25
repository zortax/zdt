-- Worktree threads, and the checkpoints that bracket every turn.

ALTER TABLE threads ADD COLUMN worktree TEXT NOT NULL DEFAULT '';
ALTER TABLE threads ADD COLUMN branch TEXT NOT NULL DEFAULT '';
ALTER TABLE threads ADD COLUMN base_branch TEXT NOT NULL DEFAULT '';
ALTER TABLE threads ADD COLUMN diff_files INTEGER NOT NULL DEFAULT 0;
ALTER TABLE threads ADD COLUMN diff_added INTEGER NOT NULL DEFAULT 0;
ALTER TABLE threads ADD COLUMN diff_removed INTEGER NOT NULL DEFAULT 0;

CREATE TABLE turns (
    id INTEGER PRIMARY KEY,
    thread_id INTEGER NOT NULL REFERENCES threads (id),
    first_item INTEGER NOT NULL,
    resume_before TEXT,
    before_ref TEXT NOT NULL DEFAULT '',
    after_ref TEXT NOT NULL DEFAULT '',
    created_at_ms INTEGER NOT NULL
);

CREATE INDEX turns_by_thread ON turns (thread_id);
