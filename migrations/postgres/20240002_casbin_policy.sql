-- Casbin policy table for PostgreSQL
CREATE TABLE IF NOT EXISTS casbin_rule (
    id    SERIAL PRIMARY KEY,
    ptype TEXT NOT NULL,
    v0    TEXT NOT NULL DEFAULT '',
    v1    TEXT NOT NULL DEFAULT '',
    v2    TEXT NOT NULL DEFAULT '',
    v3    TEXT NOT NULL DEFAULT '',
    v4    TEXT NOT NULL DEFAULT '',
    v5    TEXT NOT NULL DEFAULT ''
);
CREATE UNIQUE INDEX IF NOT EXISTS idx_casbin_rule
    ON casbin_rule(ptype, v0, v1, v2, v3, v4, v5);
