-- Copyright 2026 ByteBaker
-- SPDX-License-Identifier: Elastic-2.0

PRAGMA user_version = 1;
CREATE TABLE users (
    id INTEGER PRIMARY KEY,
    username TEXT NOT NULL UNIQUE,
    password_hash TEXT NOT NULL,
    is_admin INTEGER NOT NULL DEFAULT 0 CHECK (is_admin IN (0, 1)),
    enabled INTEGER NOT NULL DEFAULT 1 CHECK (enabled IN (0, 1)),
    CHECK (id != 1 OR (is_admin = 1 AND enabled = 1))
);
CREATE TABLE projects (name TEXT PRIMARY KEY);
INSERT INTO projects VALUES ('default');
CREATE TABLE environments (
    name TEXT PRIMARY KEY,
    project TEXT NOT NULL REFERENCES projects(name),
    revision INTEGER NOT NULL DEFAULT 0 CHECK (revision >= 0)
);
INSERT INTO environments(name, project) VALUES ('development', 'default'), ('production', 'default');
CREATE TABLE permissions (
    user_id INTEGER NOT NULL REFERENCES users(id),
    environment TEXT NOT NULL REFERENCES environments(name),
    role TEXT NOT NULL CHECK (role IN ('viewer', 'editor', 'manager')),
    PRIMARY KEY (user_id, environment)
);
CREATE TABLE flags (
    environment TEXT NOT NULL REFERENCES environments(name),
    key TEXT NOT NULL,
    config TEXT NOT NULL CHECK (json_valid(config)),
    PRIMARY KEY (environment, key)
);
CREATE TABLE applications (
    id TEXT PRIMARY KEY,
    environment TEXT NOT NULL REFERENCES environments(name),
    name TEXT NOT NULL,
    revision INTEGER NOT NULL DEFAULT 0 CHECK (revision >= 0),
    UNIQUE (environment, name),
    UNIQUE (environment, id)
);
CREATE TABLE application_flags (
    environment TEXT NOT NULL,
    application_id TEXT NOT NULL,
    flag_key TEXT NOT NULL,
    PRIMARY KEY (application_id, flag_key),
    FOREIGN KEY (environment, application_id) REFERENCES applications(environment, id),
    FOREIGN KEY (environment, flag_key) REFERENCES flags(environment, key) ON DELETE CASCADE
);
CREATE TABLE sdk_keys (
    id TEXT PRIMARY KEY,
    application_id TEXT NOT NULL REFERENCES applications(id),
    secret TEXT UNIQUE,
    secret_hash TEXT NOT NULL UNIQUE,
    masked_secret TEXT NOT NULL,
    name TEXT NOT NULL,
    created_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now')),
    revoked_at TEXT,
    CHECK ((revoked_at IS NULL AND secret IS NOT NULL) OR (revoked_at IS NOT NULL AND secret IS NULL))
);
CREATE TABLE audits (
    id INTEGER PRIMARY KEY,
    environment TEXT NOT NULL REFERENCES environments(name),
    actor TEXT NOT NULL,
    action TEXT NOT NULL,
    item_type TEXT NOT NULL,
    item_id TEXT NOT NULL,
    item_name TEXT NOT NULL,
    application_id TEXT,
    details TEXT NOT NULL CHECK (json_valid(details)),
    created_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now'))
);
CREATE INDEX audits_environment ON audits(environment, id);
