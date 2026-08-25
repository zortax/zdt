CREATE TABLE projects (
    id INTEGER PRIMARY KEY,
    root TEXT NOT NULL UNIQUE,
    name TEXT NOT NULL,
    created_at_ms INTEGER NOT NULL
);

CREATE TABLE threads (
    id INTEGER PRIMARY KEY,
    project_id INTEGER NOT NULL REFERENCES projects (id),
    title TEXT NOT NULL,
    state TEXT NOT NULL,
    last_error TEXT,
    provider TEXT NOT NULL DEFAULT 'claude',
    resume TEXT,
    created_at_ms INTEGER NOT NULL,
    updated_at_ms INTEGER NOT NULL
);

CREATE TABLE messages (
    id INTEGER PRIMARY KEY,
    thread_id INTEGER NOT NULL REFERENCES threads (id),
    role TEXT NOT NULL,
    text TEXT NOT NULL,
    created_at_ms INTEGER NOT NULL
);

CREATE INDEX messages_by_thread ON messages (thread_id);
CREATE INDEX threads_by_project ON threads (project_id);
