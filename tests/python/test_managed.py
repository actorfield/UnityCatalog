"""
Tests for #1143 — Managed table staging flow and storage root resolution.

Covers:
  - MANAGED volume auto-derives storage_location from catalog storage_root
  - MANAGED table via staging: POST /staging-tables → POST /tables (MANAGED)
  - Staging table cannot be committed twice
  - MANAGED table without storage_root falls back to temp location
  - Model version auto-derives storage_location from model storage
"""
import os
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
    CreateRegisteredModel,
    CreateModelVersion,
)

UC_HOST = os.environ.get("UC_HOST", "http://localhost:8080")
UC_BASE = f"{UC_HOST}/api/2.1/unity-catalog"


def post(path, data):
    r = requests.post(f"{UC_BASE}{path}", json=data, timeout=10)
    r.raise_for_status()
    return r.json()


def get(path):
    r = requests.get(f"{UC_BASE}{path}", timeout=10)
    r.raise_for_status()
    return r.json()


# ── MANAGED Volume — storage root auto-resolution ─────────────────────────────

@pytest.mark.asyncio
async def test_managed_volume_auto_derives_location_from_catalog_storage_root(volumes_api):
    """
    When catalog has storage_root set, MANAGED volume creation should
    auto-derive storage_location as <storage_root>/schemas/<id>/volumes/<id>.
    """
    # Cleanup any leftover from previous run (order: leaf → parent)
    requests.delete(f"{UC_BASE}/volumes/cat_managed_vol.s1.managed_vol")
    requests.delete(f"{UC_BASE}/schemas/cat_managed_vol.s1")
    requests.delete(f"{UC_BASE}/catalogs/cat_managed_vol")
    # Create catalog with storage_root
    cat = post("/catalogs", {"name": "cat_managed_vol", "storage_root": "file:///tmp/uc/managed"})
    post("/schemas", {"name": "s1", "catalog_name": "cat_managed_vol"})

    try:
        vol = post("/volumes", {
            "name": "managed_vol",
            "catalog_name": "cat_managed_vol",
            "schema_name": "s1",
            "volume_type": "MANAGED",
            # No storage_location — should be auto-derived
        })
        assert vol["volume_type"] == "MANAGED"
        assert vol["storage_location"] is not None
        assert "file:///tmp/uc/managed" in vol["storage_location"]
        assert "/volumes/" in vol["storage_location"]
    finally:
        requests.delete(f"{UC_BASE}/volumes/cat_managed_vol.s1.managed_vol")
        requests.delete(f"{UC_BASE}/schemas/cat_managed_vol.s1")
        requests.delete(f"{UC_BASE}/catalogs/cat_managed_vol")


@pytest.mark.asyncio
async def test_managed_volume_no_storage_root_still_creates(volumes_api):
    """MANAGED volume without storage_root still creates (location may be None)."""
    vol = post("/volumes", {
        "name": "managed_vol_no_root",
        "catalog_name": "unity",
        "schema_name": "default",
        "volume_type": "MANAGED",
    })
    try:
        assert vol["name"] == "managed_vol_no_root"
        assert vol["volume_type"] == "MANAGED"
    finally:
        requests.delete(f"{UC_BASE}/volumes/unity.default.managed_vol_no_root")


# ── MANAGED Table — full staging flow ─────────────────────────────────────────

@pytest.mark.asyncio
async def test_managed_table_via_staging_flow(tables_api):
    """
    Full managed table creation flow:
    1. POST /staging-tables → get staging_location + table_id
    2. POST /tables with table_type=MANAGED + storage_location=<staging_location>
    3. Table is created with the staging UUID as its table_id
    4. Staging table is marked committed
    """
    # Step 1: create staging table
    staging = post("/staging-tables", {
        "name": "managed_tbl",
        "catalog_name": "unity",
        "schema_name": "default",
    })
    assert "table_id" in staging
    assert "staging_location" in staging
    staging_id = staging["table_id"]
    staging_loc = staging["staging_location"]

    try:
        # Step 2: commit — create MANAGED table pointing at staging location
        table = post("/tables", {
            "name": "managed_tbl",
            "catalog_name": "unity",
            "schema_name": "default",
            "table_type": "MANAGED",
            "data_source_format": "DELTA",
            "storage_location": staging_loc,
            "columns": [
                {
                    "name": "id",
                    "type_text": "int",
                    "type_name": "INT",
                    "type_json": '{"type":"integer"}',
                    "nullable": False,
                    "position": 0,
                }
            ],
        })

        # Step 3: table ID matches staging UUID
        assert table["table_id"] == staging_id
        assert table["table_type"] == "MANAGED"
        assert table["storage_location"] is not None

        # Step 4: verify table is accessible via GET
        fetched = get("/tables/unity.default.managed_tbl")
        assert fetched["name"] == "managed_tbl"
        assert fetched["table_type"] == "MANAGED"

    finally:
        requests.delete(f"{UC_BASE}/tables/unity.default.managed_tbl")


@pytest.mark.asyncio
async def test_managed_table_staging_cannot_be_committed_twice(tables_api):
    """Committing the same staging table twice returns an error."""
    staging = post("/staging-tables", {
        "name": "double_commit_tbl",
        "catalog_name": "unity",
        "schema_name": "default",
    })
    staging_loc = staging["staging_location"]

    # First commit — should succeed
    post("/tables", {
        "name": "double_commit_tbl",
        "catalog_name": "unity",
        "schema_name": "default",
        "table_type": "MANAGED",
        "data_source_format": "DELTA",
        "storage_location": staging_loc,
    })

    try:
        # Second commit — should fail
        r = requests.post(f"{UC_BASE}/tables", json={
            "name": "double_commit_tbl2",
            "catalog_name": "unity",
            "schema_name": "default",
            "table_type": "MANAGED",
            "data_source_format": "DELTA",
            "storage_location": staging_loc,
        }, timeout=10)
        assert r.status_code in (400, 409), f"Expected error, got {r.status_code}: {r.text}"
    finally:
        requests.delete(f"{UC_BASE}/tables/unity.default.double_commit_tbl")


@pytest.mark.asyncio
async def test_managed_table_invalid_staging_location_returns_error(tables_api):
    """Creating MANAGED table with unknown staging location returns 404."""
    r = requests.post(f"{UC_BASE}/tables", json={
        "name": "bad_staging_tbl",
        "catalog_name": "unity",
        "schema_name": "default",
        "table_type": "MANAGED",
        "data_source_format": "DELTA",
        "storage_location": "file:///tmp/uc/staging/nonexistent/uuid",
    }, timeout=10)
    assert r.status_code == 404, f"Expected 404, got {r.status_code}: {r.text}"


# ── Model storage — location auto-derived ─────────────────────────────────────

@pytest.mark.asyncio
async def test_model_version_location_derived_from_model(api_client):
    """Model version storage_location is auto-derived from model's storage_location."""
    from unitycatalog.client import RegisteredModelsApi, ModelVersionsApi

    models_api = RegisteredModelsApi(api_client)
    versions_api = ModelVersionsApi(api_client)

    # Create catalog with storage_root so model gets a location
    # Cleanup: leaf objects first
    requests.delete(f"{UC_BASE}/models/cat_model_loc.s1.loc_model/versions/1")
    requests.delete(f"{UC_BASE}/models/cat_model_loc.s1.loc_model")
    requests.delete(f"{UC_BASE}/schemas/cat_model_loc.s1")
    requests.delete(f"{UC_BASE}/catalogs/cat_model_loc")
    post("/catalogs", {"name": "cat_model_loc", "storage_root": "file:///tmp/uc/models"})
    post("/schemas", {"name": "s1", "catalog_name": "cat_model_loc"})

    try:
        model = await models_api.create_registered_model(CreateRegisteredModel(
            name="loc_model",
            catalog_name="cat_model_loc",
            schema_name="s1",
        ))
        assert model.storage_location is not None
        assert "file:///tmp/uc/models" in model.storage_location

        version = await versions_api.create_model_version(CreateModelVersion(
            model_name="loc_model",
            catalog_name="cat_model_loc",
            schema_name="s1",
            source="s3://ml-runs/run1/artifacts",
        ))
        # Version location should be under the model location
        assert version.storage_location is not None
        assert model.storage_location in version.storage_location
        assert "/versions/" in version.storage_location

    finally:
        requests.delete(f"{UC_BASE}/models/cat_model_loc.s1.loc_model/versions/1")
        requests.delete(f"{UC_BASE}/models/cat_model_loc.s1.loc_model")
        requests.delete(f"{UC_BASE}/schemas/cat_model_loc.s1")
        requests.delete(f"{UC_BASE}/catalogs/cat_model_loc")
