-- Copyright 2026 ByteBaker
-- SPDX-License-Identifier: Elastic-2.0

PRAGMA user_version = 1;
CREATE TABLE admin (
    id INTEGER PRIMARY KEY CHECK (id = 1),
    username TEXT NOT NULL UNIQUE,
    password_hash TEXT NOT NULL
);
CREATE TABLE projects (name TEXT PRIMARY KEY);
INSERT INTO projects VALUES ('default');
CREATE TABLE environments (
    name TEXT PRIMARY KEY,
    project TEXT NOT NULL REFERENCES projects(name),
    revision INTEGER NOT NULL DEFAULT 0 CHECK (revision >= 0)
);
INSERT INTO environments(name, project) VALUES ('development', 'default'), ('production', 'default');
CREATE TABLE flags (
    environment TEXT NOT NULL REFERENCES environments(name),
    key TEXT NOT NULL,
    config TEXT NOT NULL CHECK (json_valid(config)),
    PRIMARY KEY (environment, key)
);
CREATE TABLE sdk_keys (
    id TEXT PRIMARY KEY,
    environment TEXT NOT NULL REFERENCES environments(name),
    secret TEXT NOT NULL UNIQUE,
    name TEXT NOT NULL
);
