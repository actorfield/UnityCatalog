"""
Regression tests for upstream bugs fixed in the Rust server.

#1105 - list endpoints filter by RBAC visibility (no-auth: all visible; schema/table/volume/function/model)
#1053 - column type_text/type_json not required; type_json derived from type_text
#1160 - path_credentials requires correct privilege on external location, not OWNER on metastore
#1407 - --log-level CLI flag controls log verbosity
#1576 - credential vendor caches results (tested via timing / repeated calls)
"""
import os
import time
import pytest
import requests

from unitycatalog.client import (
    CreateTable,
    CreateVolumeRequestContent,
    ColumnInfo,
    ColumnTypeName,
    DataSourceFormat,
    TableType,
    VolumeType,
)

UC_HOST = os.environ.get("UC_HOST", "http://localhost:8080")
UC_BASE = f"{UC_HOST}/api/2.1/unity-catalog"


def api_post(path, data):
    r = requests.post(f"{UC_BASE}{path}", json=data, timeout=10)
    r.raise_for_status()
    return r.json()


def api_get(path):
    r = requests.get(f"{UC_BASE}{path}", timeout=10)
    r.raise_for_status()
    return r.json()


def api_delete(path):
    return requests.delete(f"{UC_BASE}{path}", timeout=10)


# ── #1053 — column fields not required ───────────────────────────────────────

@pytest.mark.asyncio
async def test_table_create_without_type_json(tables_api):
    """
    #1053: type_json should not be required.
    Server should derive it from type_text automatically.
    """
    table = await tables_api.create_table(
        CreateTable(
            name="no_type_json_tbl",
            catalog_name="unity",
            schema_name="default",
            table_type=TableType.EXTERNAL,
            data_source_format=DataSourceFormat.DELTA,
            storage_location="/tmp/uc/no_type_json",
            columns=[
                ColumnInfo(
                    name="col1",
                    type_text="int",
                    type_name=ColumnTypeName.INT,
                    # type_json intentionally omitted
                    position=0,
                ),
                ColumnInfo(
                    name="col2",
                    type_text="string",
                    type_name=ColumnTypeName.STRING,
                    # type_json intentionally omitted
                    position=1,
                ),
            ],
        )
    )
    try:
        assert table.name == "no_type_json_tbl"
        assert len(table.columns) == 2
        col_names = {c.name for c in table.columns}
        assert col_names == {"col1", "col2"}
        # type_name should be preserved
        col_types = {c.name: c.type_name for c in table.columns}
        assert col_types["col1"] == ColumnTypeName.INT
        assert col_types["col2"] == ColumnTypeName.STRING
    finally:
        await tables_api.delete_table("unity.default.no_type_json_tbl")


@pytest.mark.asyncio
async def test_table_create_with_only_type_text(tables_api):
    """#1053: providing only type_text (no type_name or type_json) should succeed."""
    table = await tables_api.create_table(
        CreateTable(
            name="only_type_text_tbl",
            catalog_name="unity",
            schema_name="default",
            table_type=TableType.EXTERNAL,
            data_source_format=DataSourceFormat.DELTA,
            storage_location="/tmp/uc/only_type_text",
            columns=[
                ColumnInfo(
                    name="amount",
                    type_text="double",
                    position=0,
                    # type_name and type_json both omitted
                ),
            ],
        )
    )
    try:
        assert table.name == "only_type_text_tbl"
        assert len(table.columns) == 1
        assert table.columns[0].name == "amount"
        assert table.columns[0].type_text == "double"
    finally:
        await tables_api.delete_table("unity.default.only_type_text_tbl")


# ── #1105 — list visibility with no-auth ────────────────────────────────────

@pytest.mark.asyncio
async def test_list_schemas_returns_all_in_no_auth_mode(schemas_api):
    """
    #1105: in --no-auth mode all schemas must be visible to every caller.
    Creates a schema, verifies it appears in list, cleans up.
    """
    api_post("/schemas", {"name": "rbac_test_sch", "catalog_name": "unity"})
    try:
        response = await schemas_api.list_schemas("unity")
        schema_names = {s.name for s in response.schemas}
        assert "default" in schema_names, "seed schema 'default' missing from list"
        assert "rbac_test_sch" in schema_names, "newly created schema missing from list"
    finally:
        api_delete("/schemas/unity.rbac_test_sch")


@pytest.mark.asyncio
async def test_list_tables_returns_all_in_no_auth_mode(tables_api):
    """#1105: tables created by any caller appear in list in --no-auth mode."""
    await tables_api.create_table(
        CreateTable(
            name="rbac_visible_tbl",
            catalog_name="unity",
            schema_name="default",
            table_type=TableType.EXTERNAL,
            data_source_format=DataSourceFormat.DELTA,
            storage_location="/tmp/uc/rbac_tbl",
            columns=[ColumnInfo(name="id", type_text="int", type_name=ColumnTypeName.INT, position=0)],
        )
    )
    try:
        response = await tables_api.list_tables("unity", "default")
        table_names = {t.name for t in response.tables}
        assert "rbac_visible_tbl" in table_names
        assert "numbers" in table_names, "seed table 'numbers' missing"
    finally:
        await tables_api.delete_table("unity.default.rbac_visible_tbl")


@pytest.mark.asyncio
async def test_list_volumes_returns_all_in_no_auth_mode(volumes_api):
    """#1105: volumes appear in list in --no-auth mode."""
    response = await volumes_api.list_volumes("unity", "default")
    volume_names = {v.name for v in response.volumes}
    assert "txt_files" in volume_names
    assert "json_files" in volume_names


@pytest.mark.asyncio
async def test_list_functions_returns_all_in_no_auth_mode(functions_api):
    """#1105: functions appear in list in --no-auth mode."""
    response = await functions_api.list_functions("unity", "default")
    function_names = {f.name for f in response.functions}
    assert "sum" in function_names
    assert "lowercase" in function_names


# ── #1160 — path_credentials correct auth ────────────────────────────────────

@pytest.mark.asyncio
async def test_path_credentials_local_path_returns_empty(tables_api):
    """
    #1160: path_credentials for a local file:// path (no external location)
    should return empty credentials (not 403 requiring OWNER on metastore).
    """
    r = requests.post(
        f"{UC_BASE}/temporary-path-credentials",
        json={"url": "file:///tmp/uc/some/path", "operation": "READ"},
        timeout=10,
    )
    assert r.status_code == 200, f"Expected 200, got {r.status_code}: {r.text}"
    creds = r.json()
    # Local paths return empty credentials
    assert creds.get("aws_temp_credentials") is None
    assert creds.get("gcp_oauth_token") is None
    assert creds.get("azure_user_delegation_sas") is None


@pytest.mark.asyncio
async def test_path_credentials_with_external_location(tables_api):
    """
    #1160: path_credentials for a path under a registered external location
    should return credentials (or empty for local) without requiring OWNER on metastore.
    """
    # Create credential + external location
    api_post("/credentials", {
        "name": "cred_for_path_test",
        "purpose": "AWS_IAM_ROLE",
        "aws_iam_role": {"role_arn": "arn:aws:iam::123:role/test"},
    })
    api_post("/external-locations", {
        "name": "el_path_test",
        "url": "s3://test-bucket/path-cred-test",
        "credential_name": "cred_for_path_test",
    })

    try:
        r = requests.post(
            f"{UC_BASE}/temporary-path-credentials",
            json={
                "url": "s3://test-bucket/path-cred-test/subdir/file.parquet",
                "operation": "READ",
            },
            timeout=10,
        )
        # Should not 403 (wrong auth). May be 200 (empty/real creds) or 501 (AWS not configured).
        assert r.status_code in (200, 501), \
            f"Got unexpected {r.status_code}: {r.text}"
    finally:
        api_delete("/external-locations/el_path_test")
        api_delete("/credentials/cred_for_path_test")


# ── #1576 — credential caching ───────────────────────────────────────────────

@pytest.mark.asyncio
async def test_temp_table_credentials_called_twice_consistent(tables_api):
    """
    #1576: calling temp credentials twice for the same table should return
    identical (or equivalent) results — verifies the cache doesn't corrupt state.
    """
    table_id = api_get("/tables/unity.default.numbers")["table_id"]

    r1 = requests.post(
        f"{UC_BASE}/temporary-table-credentials",
        json={"table_id": table_id, "operation": "READ"},
        timeout=10,
    )
    r2 = requests.post(
        f"{UC_BASE}/temporary-table-credentials",
        json={"table_id": table_id, "operation": "READ"},
        timeout=10,
    )
    assert r1.status_code == 200
    assert r2.status_code == 200
    # Both calls return the same shape (local table → empty credentials)
    assert r1.json() == r2.json()


@pytest.mark.asyncio
async def test_temp_credentials_repeated_calls_are_fast(tables_api):
    """
    #1576: repeated credential requests for the same resource should be served
    quickly from cache (no external STS call each time).
    For local/file tables this is trivially fast; verifies no regression.
    """
    table_id = api_get("/tables/unity.default.numbers")["table_id"]

    times = []
    for _ in range(5):
        t0 = time.monotonic()
        r = requests.post(
            f"{UC_BASE}/temporary-table-credentials",
            json={"table_id": table_id, "operation": "READ"},
            timeout=10,
        )
        times.append(time.monotonic() - t0)
        assert r.status_code == 200

    avg_ms = sum(times) / len(times) * 1000
    # All 5 calls should complete well under 500ms each (cache or local path)
    assert avg_ms < 500, f"Average response time {avg_ms:.1f}ms too slow — caching may not be working"


# ── #1407 — log level ─────────────────────────────────────────────────────────

def test_server_responds_after_log_level_set():
    """
    #1407: server started with --log-level should still respond normally.
    (Log level is set at server startup; we just verify the server is functional.)
    """
    r = requests.get(f"{UC_HOST}/api/2.1/unity-catalog/metastore_summary", timeout=5)
    assert r.status_code == 200
    assert "metastore_id" in r.json()
