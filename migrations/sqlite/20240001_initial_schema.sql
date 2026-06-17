-- Unity Catalog SQLite schema
-- UUIDs stored as BLOB(16) via sqlx uuid feature
-- Timestamps stored as INTEGER (epoch milliseconds)

CREATE TABLE IF NOT EXISTS uc_metastore (
    id   BLOB NOT NULL PRIMARY KEY,
    name TEXT NOT NULL
);

CREATE TABLE IF NOT EXISTS uc_catalogs (
    id               BLOB    NOT NULL PRIMARY KEY,
    name             TEXT    NOT NULL UNIQUE,
    comment          TEXT,
    owner            TEXT,
    created_at       INTEGER NOT NULL,
    created_by       TEXT,
    updated_at       INTEGER,
    updated_by       TEXT,
    storage_root     TEXT,
    storage_location TEXT
);

CREATE TABLE IF NOT EXISTS uc_schemas (
    id               BLOB    NOT NULL PRIMARY KEY,
    catalog_id       BLOB    NOT NULL REFERENCES uc_catalogs(id),
    name             TEXT    NOT NULL,
    comment          TEXT,
    owner            TEXT,
    created_at       INTEGER NOT NULL,
    created_by       TEXT,
    updated_at       INTEGER,
    updated_by       TEXT,
    storage_root     TEXT,
    storage_location TEXT,
    UNIQUE(catalog_id, name)
);

CREATE TABLE IF NOT EXISTS uc_tables (
    id                                      BLOB    NOT NULL PRIMARY KEY,
    schema_id                               BLOB    NOT NULL REFERENCES uc_schemas(id),
    name                                    TEXT    NOT NULL,
    type                                    TEXT    NOT NULL,
    owner                                   TEXT,
    created_at                              INTEGER NOT NULL,
    created_by                              TEXT,
    updated_at                              INTEGER,
    updated_by                              TEXT,
    data_source_format                      TEXT,
    comment                                 TEXT,
    url                                     TEXT,
    column_count                            INTEGER,
    view_definition                         TEXT,
    uniform_iceberg_metadata_location       TEXT,
    uniform_iceberg_converted_delta_version INTEGER,
    uniform_iceberg_converted_delta_timestamp INTEGER,
    UNIQUE(schema_id, name)
);
CREATE INDEX IF NOT EXISTS idx_uc_tables_name ON uc_tables(name);

CREATE TABLE IF NOT EXISTS uc_columns (
    id                 BLOB    NOT NULL PRIMARY KEY,
    table_id           BLOB    NOT NULL REFERENCES uc_tables(id),
    name               TEXT    NOT NULL,
    ordinal_position   INTEGER NOT NULL,
    type_text          TEXT    NOT NULL,
    type_json          TEXT    NOT NULL,
    type_name          TEXT    NOT NULL,
    type_precision     INTEGER,
    type_scale         INTEGER,
    type_interval_type TEXT,
    nullable           INTEGER NOT NULL DEFAULT 0,
    comment            TEXT,
    partition_index    INTEGER,
    UNIQUE(table_id, ordinal_position)
);

CREATE TABLE IF NOT EXISTS uc_volumes (
    id               BLOB    NOT NULL PRIMARY KEY,
    schema_id        BLOB    NOT NULL REFERENCES uc_schemas(id),
    name             TEXT    NOT NULL,
    comment          TEXT,
    storage_location TEXT,
    owner            TEXT,
    created_at       INTEGER NOT NULL,
    created_by       TEXT,
    updated_at       INTEGER,
    updated_by       TEXT,
    volume_type      TEXT    NOT NULL,
    UNIQUE(schema_id, name)
);

CREATE TABLE IF NOT EXISTS uc_functions (
    id                  BLOB    NOT NULL PRIMARY KEY,
    schema_id           BLOB    NOT NULL REFERENCES uc_schemas(id),
    name                TEXT    NOT NULL,
    comment             TEXT,
    owner               TEXT,
    created_at          INTEGER,
    created_by          TEXT,
    updated_at          INTEGER,
    updated_by          TEXT,
    data_type           TEXT,
    full_data_type      TEXT,
    external_language   TEXT,
    is_deterministic    INTEGER,
    is_null_call        INTEGER,
    parameter_style     TEXT,
    routine_body        TEXT,
    routine_definition  TEXT,
    sql_data_access     TEXT,
    security_type       TEXT,
    specific_name       TEXT,
    UNIQUE(schema_id, name)
);

CREATE TABLE IF NOT EXISTS uc_function_parameters (
    id                 BLOB    NOT NULL PRIMARY KEY,
    function_id        BLOB    NOT NULL REFERENCES uc_functions(id),
    name               TEXT    NOT NULL,
    input_or_return    INTEGER NOT NULL,    -- 0=INPUT, 1=RETURN
    ordinal_position   INTEGER NOT NULL,
    type_text          TEXT,
    type_json          TEXT,
    type_name          TEXT,
    type_precision     INTEGER,
    type_scale         INTEGER,
    type_interval_type TEXT,
    comment            TEXT,
    parameter_mode     TEXT,
    parameter_default  TEXT
);

CREATE TABLE IF NOT EXISTS uc_registered_models (
    id               BLOB    NOT NULL PRIMARY KEY,
    schema_id        BLOB    NOT NULL REFERENCES uc_schemas(id),
    name             TEXT    NOT NULL,
    owner            TEXT,
    created_at       INTEGER,
    created_by       TEXT,
    updated_at       INTEGER,
    updated_by       TEXT,
    comment          TEXT,
    url              TEXT,
    max_version_number INTEGER,
    UNIQUE(schema_id, name)
);
CREATE INDEX IF NOT EXISTS idx_uc_registered_models_name ON uc_registered_models(name);

CREATE TABLE IF NOT EXISTS uc_model_versions (
    id                   BLOB    NOT NULL PRIMARY KEY,
    registered_model_id  BLOB    NOT NULL REFERENCES uc_registered_models(id),
    version              INTEGER,
    source               TEXT,
    run_id               TEXT,
    status               TEXT,
    owner                TEXT,
    created_at           INTEGER,
    created_by           TEXT,
    updated_at           INTEGER,
    updated_by           TEXT,
    comment              TEXT,
    url                  TEXT
);
CREATE INDEX IF NOT EXISTS idx_uc_model_versions ON uc_model_versions(registered_model_id, version);

CREATE TABLE IF NOT EXISTS uc_staging_tables (
    id                  BLOB    NOT NULL PRIMARY KEY,
    schema_id           BLOB    NOT NULL REFERENCES uc_schemas(id),
    name                TEXT    NOT NULL,
    staging_location    TEXT    NOT NULL,
    created_at          INTEGER NOT NULL,
    created_by          TEXT,
    accessed_at         INTEGER NOT NULL,
    stage_committed     INTEGER NOT NULL DEFAULT 0,
    stage_committed_at  INTEGER,
    purge_state         INTEGER NOT NULL DEFAULT 0,
    num_cleanup_retries INTEGER NOT NULL DEFAULT 0,
    last_cleanup_at     INTEGER
);
CREATE INDEX IF NOT EXISTS idx_uc_staging_tables_location ON uc_staging_tables(staging_location);

CREATE TABLE IF NOT EXISTS uc_delta_commits (
    id                                  BLOB    NOT NULL PRIMARY KEY,
    table_id                            BLOB    NOT NULL REFERENCES uc_tables(id),
    commit_version                      INTEGER NOT NULL,
    commit_filename                     TEXT    NOT NULL,
    commit_filesize                     INTEGER NOT NULL,
    commit_file_modification_timestamp  INTEGER NOT NULL,
    commit_timestamp                    INTEGER NOT NULL,
    is_backfilled_latest_commit         INTEGER NOT NULL DEFAULT 0,
    UNIQUE(table_id, commit_version)
);

CREATE TABLE IF NOT EXISTS uc_users (
    id          BLOB NOT NULL PRIMARY KEY,
    name        TEXT NOT NULL UNIQUE,
    email       TEXT,
    external_id TEXT,
    state       TEXT,
    created_at  INTEGER,
    updated_at  INTEGER,
    picture_url TEXT
);

CREATE TABLE IF NOT EXISTS uc_credentials (
    id              BLOB    NOT NULL PRIMARY KEY,
    name            TEXT    NOT NULL UNIQUE,
    credential_type TEXT    NOT NULL,
    credential      TEXT    NOT NULL,
    purpose         TEXT    NOT NULL,
    comment         TEXT,
    owner           TEXT,
    created_at      INTEGER NOT NULL,
    created_by      TEXT,
    updated_at      INTEGER,
    updated_by      TEXT
);

CREATE TABLE IF NOT EXISTS uc_external_locations (
    id            BLOB    NOT NULL PRIMARY KEY,
    name          TEXT    NOT NULL UNIQUE,
    url           TEXT    NOT NULL,
    comment       TEXT,
    owner         TEXT,
    credential_id BLOB    NOT NULL REFERENCES uc_credentials(id),
    created_at    INTEGER,
    created_by    TEXT,
    updated_at    INTEGER,
    updated_by    TEXT
);
CREATE INDEX IF NOT EXISTS idx_uc_ext_loc_url ON uc_external_locations(url);
CREATE INDEX IF NOT EXISTS idx_uc_ext_loc_cred ON uc_external_locations(credential_id);

CREATE TABLE IF NOT EXISTS uc_properties (
    id             BLOB NOT NULL PRIMARY KEY,
    entity_id      BLOB NOT NULL,
    entity_type    TEXT NOT NULL,
    property_key   TEXT NOT NULL,
    property_value TEXT NOT NULL,
    UNIQUE(entity_id, entity_type, property_key)
);
CREATE INDEX IF NOT EXISTS idx_uc_properties_entity ON uc_properties(entity_id, entity_type);

CREATE TABLE IF NOT EXISTS uc_dependencies (
    id               BLOB NOT NULL PRIMARY KEY,
    dependent_type   TEXT NOT NULL,
    dependent_id     BLOB NOT NULL,
    dependency_type  TEXT NOT NULL,
    dependency_catalog TEXT,
    dependency_schema  TEXT,
    dependency_name    TEXT
);
CREATE INDEX IF NOT EXISTS idx_uc_dependencies_dependent ON uc_dependencies(dependent_type, dependent_id);
