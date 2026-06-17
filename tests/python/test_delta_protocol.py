"""
Integration tests for the Delta Lake CCv2 protocol API (/delta/v1/*).
Each test uses a unique table name derived from _RUN_ID to avoid conflicts
when the same DB is reused across test runs.
"""
import os
import uuid
import pytest
import requests

UC_HOST = os.environ.get("UC_HOST", "http://localhost:8080")
DELTA = f"{UC_HOST}/delta/v1"
UC_BASE = f"{UC_HOST}/api/2.1/unity-catalog"
_RUN_ID = uuid.uuid4().hex[:6]


def delta_get(path, params=None):
    r = requests.get(f"{DELTA}{path}", params=params, timeout=10)
    r.raise_for_status()
    return r.json()


def delta_post(path, data):
    r = requests.post(f"{DELTA}{path}", json=data, timeout=10)
    r.raise_for_status()
    return r.json()


def delta_delete(path):
    return requests.delete(f"{DELTA}{path}", timeout=10)


def tbl_path(name):
    return f"/catalogs/unity/schemas/default/tables/{name}"


@pytest.mark.asyncio
async def test_delta_config_returns_protocol_version(api_client):
    result = delta_get("/config")
    assert "protocol-version" in result
    assert isinstance(result["endpoints"], list)


@pytest.mark.asyncio
async def test_delta_create_and_load_table(api_client):
    name = f"d_create_{_RUN_ID}"
    tbl = delta_post("/catalogs/unity/schemas/default/tables", {
        "name": name, "location": "s3://bucket/delta-create", "table-type": "EXTERNAL",
    })
    try:
        assert tbl["metadata"]["table-type"] == "EXTERNAL"
        loaded = delta_get(tbl_path(name))
        assert loaded["metadata"]["table-uuid"] is not None
        assert loaded["latest-table-version"] == 0
    finally:
        delta_delete(tbl_path(name))


@pytest.mark.asyncio
async def test_delta_table_exists(api_client):
    name = f"d_exists_{_RUN_ID}"
    delta_post("/catalogs/unity/schemas/default/tables", {
        "name": name, "location": "s3://bucket/delta-exists", "table-type": "EXTERNAL",
    })
    try:
        r = requests.head(f"{DELTA}{tbl_path(name)}", timeout=10)
        assert r.status_code == 200
        r2 = requests.head(f"{DELTA}/catalogs/unity/schemas/default/tables/does_not_exist_xyz", timeout=10)
        assert r2.status_code == 404
    finally:
        delta_delete(tbl_path(name))


@pytest.mark.asyncio
async def test_delta_update_table_set_properties(api_client):
    name = f"d_props_{_RUN_ID}"
    delta_post("/catalogs/unity/schemas/default/tables", {
        "name": name, "location": "s3://bucket/delta-props", "table-type": "EXTERNAL",
    })
    try:
        result = delta_post(tbl_path(name), {
            "updates": [{"action": "set-properties", "updates": {"owner": "alice"}}]
        })
        assert "metadata" in result
    finally:
        delta_delete(tbl_path(name))


@pytest.mark.asyncio
async def test_delta_update_table_add_commit(api_client):
    name = f"d_commit_{_RUN_ID}"
    delta_post("/catalogs/unity/schemas/default/tables", {
        "name": name, "location": "s3://bucket/delta-commit", "table-type": "EXTERNAL",
    })
    try:
        r1 = delta_post(tbl_path(name), {"updates": [{"action": "add-commit", "commit": {
            "version": 1, "timestamp": 1700000000000,
            "file-name": "00000000000000000001.json", "file-size": 512,
            "file-modification-timestamp": 1700000000000,
        }}]})
        assert r1["latest-table-version"] == 1

        r2 = delta_post(tbl_path(name), {"updates": [{"action": "add-commit", "commit": {
            "version": 2, "timestamp": 1700000001000,
            "file-name": "00000000000000000002.json", "file-size": 1024,
            "file-modification-timestamp": 1700000001000,
        }}]})
        assert r2["latest-table-version"] == 2
    finally:
        delta_delete(tbl_path(name))


@pytest.mark.asyncio
async def test_delta_add_commit_version_conflict(api_client):
    name = f"d_conflict_{_RUN_ID}"
    delta_post("/catalogs/unity/schemas/default/tables", {
        "name": name, "location": "s3://bucket/delta-conflict", "table-type": "EXTERNAL",
    })
    try:
        delta_post(tbl_path(name), {"updates": [{"action": "add-commit", "commit": {
            "version": 5, "timestamp": 1700000000000,
            "file-name": "00000000000000000005.json", "file-size": 100,
            "file-modification-timestamp": 1700000000000,
        }}]})
        r = requests.post(f"{DELTA}{tbl_path(name)}", json={"updates": [{"action": "add-commit", "commit": {
            "version": 3, "timestamp": 1700000000000,
            "file-name": "00000000000000000003.json", "file-size": 100,
            "file-modification-timestamp": 1700000000000,
        }}]}, timeout=10)
        assert r.status_code == 409, f"Expected 409, got {r.status_code}: {r.text}"
    finally:
        delta_delete(tbl_path(name))


@pytest.mark.asyncio
async def test_delta_rename_table(api_client):
    src = f"d_rename_src_{_RUN_ID}"
    dst = f"d_rename_dst_{_RUN_ID}"
    delta_post("/catalogs/unity/schemas/default/tables", {
        "name": src, "location": "s3://bucket/rename", "table-type": "EXTERNAL",
    })
    try:
        result = delta_post(f"{tbl_path(src)}/rename", {"new-name": dst})
        assert "metadata" in result
        r = requests.get(f"{DELTA}{tbl_path(src)}", timeout=10)
        assert r.status_code == 404
    finally:
        delta_delete(tbl_path(dst))


@pytest.mark.asyncio
async def test_delta_report_metrics_returns_200(api_client):
    name = f"d_metrics_{_RUN_ID}"
    delta_post("/catalogs/unity/schemas/default/tables", {
        "name": name, "location": "s3://bucket/metrics", "table-type": "EXTERNAL",
    })
    try:
        r = requests.post(f"{DELTA}{tbl_path(name)}/metrics", json={}, timeout=10)
        assert r.status_code == 200
    finally:
        delta_delete(tbl_path(name))


@pytest.mark.asyncio
async def test_delta_delete_table(api_client):
    name = f"d_del_{_RUN_ID}"
    delta_post("/catalogs/unity/schemas/default/tables", {
        "name": name, "location": "s3://bucket/del", "table-type": "EXTERNAL",
    })
    r = delta_delete(tbl_path(name))
    assert r.status_code in (200, 204)
    r2 = requests.get(f"{DELTA}{tbl_path(name)}", timeout=10)
    assert r2.status_code == 404


@pytest.mark.asyncio
async def test_delta_staging_table_create(api_client):
    name = f"d_staging_{_RUN_ID}"
    result = delta_post("/catalogs/unity/schemas/default/staging-tables", {"name": name})
    assert "table-id" in result
    assert "location" in result


@pytest.mark.asyncio
async def test_delta_path_credentials_local_returns_200(api_client):
    r = requests.get(f"{DELTA}/temporary-path-credentials",
                     params={"path": "file:///tmp/test/path", "operation": "READ"}, timeout=10)
    assert r.status_code == 200
    assert "storage-credentials" in r.json()
