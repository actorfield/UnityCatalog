"""
Regression tests for all gaps identified in cross-repo audit.

Covers:
  - force param on DELETE /catalogs and DELETE /schemas (with child checks)
  - Delta DeltaTableRequirement assertion validation
  - update_model new_name rename
  - external_locations credential_name in list/get/update
  - credentials aws_iam_role update
  - empty properties guard on PATCH
  - SQL object name validation
"""
import os
import uuid
import pytest
import requests

UC_HOST = os.environ.get("UC_HOST", "http://localhost:8080")
UC_BASE = f"{UC_HOST}/api/2.1/unity-catalog"
DELTA = f"{UC_HOST}/delta/v1"
_RUN = uuid.uuid4().hex[:6]


def post(path, data, base=UC_BASE):
    r = requests.post(f"{base}{path}", json=data, timeout=10)
    r.raise_for_status()
    return r.json()


def get(path, base=UC_BASE):
    r = requests.get(f"{base}{path}", timeout=10)
    r.raise_for_status()
    return r.json()


def patch(path, data, base=UC_BASE):
    r = requests.patch(f"{base}{path}", json=data, timeout=10)
    r.raise_for_status()
    return r.json()


def delete(path, params=None, base=UC_BASE):
    return requests.delete(f"{base}{path}", params=params, timeout=10)


# ── Force param — Catalogs ────────────────────────────────────────────────────

@pytest.mark.asyncio
async def test_delete_empty_catalog_without_force_succeeds(api_client):
    """An empty catalog can be deleted without force=true."""
    cat_name = f"gap_cat_empty_{_RUN}"
    post("/catalogs", {"name": cat_name})
    r = delete(f"/catalogs/{cat_name}")
    assert r.status_code == 200, f"Expected 200 for empty catalog delete, got {r.status_code}: {r.text}"


@pytest.mark.asyncio
async def test_delete_catalog_with_schemas_without_force_returns_409(api_client):
    """DELETE /catalogs/:name fails with 409 when schemas exist and force is not set."""
    cat_name = f"gap_cat_notempty_{_RUN}"
    post("/catalogs", {"name": cat_name})
    post("/schemas", {"name": "s1", "catalog_name": cat_name})
    try:
        r = delete(f"/catalogs/{cat_name}")
        assert r.status_code in (409, 400), f"Expected 409/400, got {r.status_code}: {r.text}"
    finally:
        delete(f"/schemas/{cat_name}.s1")
        delete(f"/catalogs/{cat_name}")


@pytest.mark.asyncio
async def test_delete_catalog_with_schemas_force_true_deletes_all(api_client):
    """DELETE /catalogs/:name?force=true cascades to schemas and their children."""
    cat_name = f"gap_cat_force_{_RUN}"
    post("/catalogs", {"name": cat_name})
    post("/schemas", {"name": "s1", "catalog_name": cat_name})
    post("/schemas", {"name": "s2", "catalog_name": cat_name})
    # Add a table inside s1
    post("/tables", {
        "name": "t1", "catalog_name": cat_name, "schema_name": "s1",
        "table_type": "EXTERNAL", "data_source_format": "DELTA",
        "storage_location": "/tmp/gap_test", "columns": [],
    })

    r = delete(f"/catalogs/{cat_name}", params={"force": "true"})
    assert r.status_code == 200, f"force delete failed: {r.status_code}: {r.text}"

    # Catalog should be gone
    r2 = requests.get(f"{UC_BASE}/catalogs/{cat_name}", timeout=10)
    assert r2.status_code == 404


# ── Force param — Schemas ─────────────────────────────────────────────────────

@pytest.mark.asyncio
async def test_delete_empty_schema_without_force_succeeds(api_client):
    """An empty schema can be deleted without force=true."""
    sch_name = f"gap_sch_empty_{_RUN}"
    post("/schemas", {"name": sch_name, "catalog_name": "unity"})
    r = delete(f"/schemas/unity.{sch_name}")
    assert r.status_code == 200, f"Expected 200 for empty schema delete, got {r.status_code}: {r.text}"


@pytest.mark.asyncio
async def test_delete_schema_with_tables_without_force_returns_409(api_client):
    """DELETE /schemas/:full_name fails with 409 when tables exist and force is not set."""
    sch_name = f"gap_sch_tables_{_RUN}"
    post("/schemas", {"name": sch_name, "catalog_name": "unity"})
    post("/tables", {
        "name": "t1", "catalog_name": "unity", "schema_name": sch_name,
        "table_type": "EXTERNAL", "data_source_format": "DELTA",
        "storage_location": "/tmp/gap_test",
        "columns": [{"name": "id", "type_text": "int", "type_name": "INT", "type_json": "{}", "nullable": False, "position": 0}],
    })
    try:
        r = delete(f"/schemas/unity.{sch_name}")
        assert r.status_code in (409, 400), f"Expected 409/400, got {r.status_code}: {r.text}"
    finally:
        delete(f"/tables/unity.{sch_name}.t1")
        delete(f"/schemas/unity.{sch_name}")


@pytest.mark.asyncio
async def test_delete_schema_with_volumes_without_force_returns_409(api_client):
    """DELETE /schemas/:full_name fails with 409 when volumes exist and force is not set."""
    sch_name = f"gap_sch_vols_{_RUN}"
    post("/schemas", {"name": sch_name, "catalog_name": "unity"})
    post("/volumes", {
        "name": "v1", "catalog_name": "unity", "schema_name": sch_name,
        "volume_type": "EXTERNAL", "storage_location": "/tmp/gap_vol",
    })
    try:
        r = delete(f"/schemas/unity.{sch_name}")
        assert r.status_code in (409, 400), f"Expected 409/400, got {r.status_code}: {r.text}"
    finally:
        delete(f"/volumes/unity.{sch_name}.v1")
        delete(f"/schemas/unity.{sch_name}")


@pytest.mark.asyncio
async def test_delete_schema_with_functions_without_force_returns_409(api_client):
    """DELETE /schemas/:full_name fails with 409 when functions exist and force is not set."""
    sch_name = f"gap_sch_funcs_{_RUN}"
    post("/schemas", {"name": sch_name, "catalog_name": "unity"})
    post("/functions", {"function_info": {
        "name": "f1", "catalog_name": "unity", "schema_name": sch_name,
        "data_type": "INT", "full_data_type": "int", "routine_body": "EXTERNAL",
        "routine_definition": "return 1", "parameter_style": "S", "is_deterministic": True,
        "sql_data_access": "NO_SQL", "is_null_call": False, "security_type": "DEFINER",
        "specific_name": "f1", "external_language": "python", "input_params": {"parameters": []},
    }})
    try:
        r = delete(f"/schemas/unity.{sch_name}")
        assert r.status_code in (409, 400), f"Expected 409/400, got {r.status_code}: {r.text}"
    finally:
        delete(f"/functions/unity.{sch_name}.f1")
        delete(f"/schemas/unity.{sch_name}")


@pytest.mark.asyncio
async def test_delete_schema_with_tables_force_true_deletes_all(api_client):
    """DELETE /schemas/:full_name?force=true cascades to tables and volumes."""
    sch_name = f"gap_sch_force_{_RUN}"
    post("/schemas", {"name": sch_name, "catalog_name": "unity"})
    post("/tables", {
        "name": "t1", "catalog_name": "unity", "schema_name": sch_name,
        "table_type": "EXTERNAL", "data_source_format": "DELTA",
        "storage_location": "/tmp/gap_force",
        "columns": [{"name": "id", "type_text": "int", "type_name": "INT", "type_json": "{}", "nullable": False, "position": 0}],
    })
    post("/volumes", {
        "name": "v1", "catalog_name": "unity", "schema_name": sch_name,
        "volume_type": "EXTERNAL", "storage_location": "/tmp/gap_force_vol",
    })

    r = delete(f"/schemas/unity.{sch_name}", params={"force": "true"})
    assert r.status_code == 200, f"force schema delete failed: {r.status_code}: {r.text}"

    r2 = requests.get(f"{UC_BASE}/schemas/unity.{sch_name}", timeout=10)
    assert r2.status_code == 404


# ── Delta Requirements ────────────────────────────────────────────────────────

@pytest.mark.asyncio
async def test_delta_update_assert_table_uuid_match_succeeds(api_client):
    """assert-table-uuid with the correct UUID allows the update."""
    tbl_name = f"d_req_ok_{_RUN}"
    created = requests.post(f"{DELTA}/catalogs/unity/schemas/default/tables", json={
        "name": tbl_name, "location": "s3://b/req-ok", "table-type": "EXTERNAL",
    }, timeout=10)
    created.raise_for_status()
    table_uuid = created.json()["metadata"]["table-uuid"]
    try:
        r = requests.post(f"{DELTA}/catalogs/unity/schemas/default/tables/{tbl_name}", json={
            "requirements": [{"type": "assert-table-uuid", "uuid": table_uuid}],
            "updates": [{"action": "set-properties", "updates": {"k": "v"}}],
        }, timeout=10)
        assert r.status_code == 200, f"Expected 200, got {r.status_code}: {r.text}"
    finally:
        requests.delete(f"{DELTA}/catalogs/unity/schemas/default/tables/{tbl_name}", timeout=10)


@pytest.mark.asyncio
async def test_delta_update_assert_table_uuid_mismatch_returns_409(api_client):
    """assert-table-uuid with wrong UUID returns 409 UpdateRequirementConflict."""
    tbl_name = f"d_req_uuid_{_RUN}"
    requests.post(f"{DELTA}/catalogs/unity/schemas/default/tables", json={
        "name": tbl_name, "location": "s3://b/req-uuid", "table-type": "EXTERNAL",
    }, timeout=10).raise_for_status()
    try:
        import uuid as _uuid
        wrong_uuid = str(_uuid.uuid4())
        r = requests.post(f"{DELTA}/catalogs/unity/schemas/default/tables/{tbl_name}", json={
            "requirements": [{"type": "assert-table-uuid", "uuid": wrong_uuid}],
            "updates": [{"action": "set-properties", "updates": {"k": "v"}}],
        }, timeout=10)
        assert r.status_code == 409, f"Expected 409, got {r.status_code}: {r.text}"
    finally:
        requests.delete(f"{DELTA}/catalogs/unity/schemas/default/tables/{tbl_name}", timeout=10)


@pytest.mark.asyncio
async def test_delta_update_assert_etag_mismatch_returns_409(api_client):
    """assert-etag with wrong value returns 409 UpdateRequirementConflict."""
    tbl_name = f"d_req_etag_{_RUN}"
    requests.post(f"{DELTA}/catalogs/unity/schemas/default/tables", json={
        "name": tbl_name, "location": "s3://b/req-etag", "table-type": "EXTERNAL",
    }, timeout=10).raise_for_status()
    try:
        r = requests.post(f"{DELTA}/catalogs/unity/schemas/default/tables/{tbl_name}", json={
            "requirements": [{"type": "assert-etag", "etag": "wrong-etag-value"}],
            "updates": [{"action": "set-properties", "updates": {"k": "v"}}],
        }, timeout=10)
        assert r.status_code == 409, f"Expected 409, got {r.status_code}: {r.text}"
    finally:
        requests.delete(f"{DELTA}/catalogs/unity/schemas/default/tables/{tbl_name}", timeout=10)


# ── Model rename ──────────────────────────────────────────────────────────────

@pytest.mark.asyncio
async def test_update_model_new_name_renames_model(api_client):
    """PATCH /models/:full_name with new_name actually renames the model in DB."""
    old_name = f"gap_mdl_old_{_RUN}"
    new_name = f"gap_mdl_new_{_RUN}"
    post("/models", {"name": old_name, "catalog_name": "unity", "schema_name": "default"})
    try:
        patch(f"/models/unity.default.{old_name}", {"new_name": new_name})
        # Old name should be gone
        r_old = requests.get(f"{UC_BASE}/models/unity.default.{old_name}", timeout=10)
        assert r_old.status_code == 404, f"Old name still exists: {r_old.status_code}"
        # New name should exist
        r_new = requests.get(f"{UC_BASE}/models/unity.default.{new_name}", timeout=10)
        assert r_new.status_code == 200, f"New name not found: {r_new.status_code}"
        assert r_new.json()["name"] == new_name
    finally:
        requests.delete(f"{UC_BASE}/models/unity.default.{new_name}", timeout=10)
        requests.delete(f"{UC_BASE}/models/unity.default.{old_name}", timeout=10)


@pytest.mark.asyncio
async def test_update_model_without_new_name_keeps_name(api_client):
    """PATCH without new_name leaves the model name unchanged."""
    mdl_name = f"gap_mdl_keep_{_RUN}"
    post("/models", {"name": mdl_name, "catalog_name": "unity", "schema_name": "default", "comment": "orig"})
    try:
        patch(f"/models/unity.default.{mdl_name}", {"comment": "updated"})
        fetched = get(f"/models/unity.default.{mdl_name}")
        assert fetched["name"] == mdl_name
        assert fetched["comment"] == "updated"
    finally:
        delete(f"/models/unity.default.{mdl_name}")


# ── External location credential_name ────────────────────────────────────────

@pytest.fixture
def gap_cred():
    """Credential for external location tests."""
    name = f"gap_cred_{_RUN}"
    post("/credentials", {"name": name, "purpose": "AWS_IAM_ROLE",
                          "aws_iam_role": {"role_arn": "arn:aws:iam::123:role/gap"}})
    yield name
    delete(f"/credentials/{name}")


@pytest.fixture
def gap_cred2():
    """Second credential for update tests."""
    name = f"gap_cred2_{_RUN}"
    post("/credentials", {"name": name, "purpose": "AWS_IAM_ROLE",
                          "aws_iam_role": {"role_arn": "arn:aws:iam::123:role/gap2"}})
    yield name
    delete(f"/credentials/{name}")


@pytest.mark.asyncio
async def test_external_location_get_returns_credential_name(gap_cred):
    """GET /external-locations/:name returns the actual credential_name, not empty string."""
    el_name = f"gap_el_get_{_RUN}"
    post("/external-locations", {"name": el_name, "url": "s3://gap/get", "credential_name": gap_cred})
    try:
        result = get(f"/external-locations/{el_name}")
        assert result["credential_name"] == gap_cred, \
            f"Expected credential_name='{gap_cred}', got '{result.get('credential_name')}'"
    finally:
        delete(f"/external-locations/{el_name}")


@pytest.mark.asyncio
async def test_external_location_list_returns_credential_names(gap_cred):
    """GET /external-locations list returns actual credential_name for each entry."""
    el_name = f"gap_el_list_{_RUN}"
    post("/external-locations", {"name": el_name, "url": "s3://gap/list", "credential_name": gap_cred})
    try:
        result = get("/external-locations")
        matching = [e for e in result["external_locations"] if e["name"] == el_name]
        assert len(matching) == 1
        assert matching[0]["credential_name"] == gap_cred, \
            f"Expected '{gap_cred}', got '{matching[0].get('credential_name')}'"
    finally:
        delete(f"/external-locations/{el_name}")


@pytest.mark.asyncio
async def test_external_location_update_credential_name_changes_credential(gap_cred, gap_cred2):
    """PATCH /external-locations/:name with credential_name updates the FK in DB."""
    el_name = f"gap_el_upd_{_RUN}"
    post("/external-locations", {"name": el_name, "url": "s3://gap/upd", "credential_name": gap_cred})
    try:
        patch(f"/external-locations/{el_name}", {"credential_name": gap_cred2})
        result = get(f"/external-locations/{el_name}")
        assert result["credential_name"] == gap_cred2, \
            f"Expected '{gap_cred2}' after update, got '{result.get('credential_name')}'"
    finally:
        delete(f"/external-locations/{el_name}")


# ── Credential aws_iam_role update ────────────────────────────────────────────

@pytest.mark.asyncio
async def test_credential_update_aws_iam_role_persists_new_arn(api_client):
    """PATCH /credentials/:name with aws_iam_role updates the role ARN in DB."""
    cred_name = f"gap_cred_arn_{_RUN}"
    post("/credentials", {"name": cred_name, "purpose": "AWS_IAM_ROLE",
                          "aws_iam_role": {"role_arn": "arn:aws:iam::123:role/old"}})
    try:
        patch(f"/credentials/{cred_name}", {
            "aws_iam_role": {"role_arn": "arn:aws:iam::123:role/new"}
        })
        result = get(f"/credentials/{cred_name}")
        assert result.get("aws_iam_role", {}).get("role_arn") == "arn:aws:iam::123:role/new", \
            f"Expected new ARN after update, got: {result.get('aws_iam_role')}"
    finally:
        delete(f"/credentials/{cred_name}")


# ── Empty properties guard ────────────────────────────────────────────────────

@pytest.mark.asyncio
async def test_patch_catalog_empty_properties_preserves_existing(api_client):
    """PATCH /catalogs/:name with empty properties={} must NOT wipe existing properties."""
    cat_name = f"gap_cat_props_{_RUN}"
    post("/catalogs", {"name": cat_name, "properties": {"existing_key": "existing_value"}})
    try:
        # Patch with empty map — should not delete existing properties
        patch(f"/catalogs/{cat_name}", {"properties": {}})
        result = get(f"/catalogs/{cat_name}")
        props = result.get("properties") or {}
        assert props.get("existing_key") == "existing_value", \
            f"Empty properties PATCH wiped existing properties. Got: {props}"
    finally:
        delete(f"/catalogs/{cat_name}")


@pytest.mark.asyncio
async def test_patch_schema_empty_properties_preserves_existing(api_client):
    """PATCH /schemas/:full_name with empty properties={} must NOT wipe existing properties."""
    sch_name = f"gap_sch_props_{_RUN}"
    post("/schemas", {"name": sch_name, "catalog_name": "unity",
                      "properties": {"existing_key": "existing_value"}})
    try:
        patch(f"/schemas/unity.{sch_name}", {"properties": {}})
        result = get(f"/schemas/unity.{sch_name}")
        props = result.get("properties") or {}
        assert props.get("existing_key") == "existing_value", \
            f"Empty properties PATCH wiped existing properties. Got: {props}"
    finally:
        delete(f"/schemas/unity.{sch_name}")


# ── SQL name validation ───────────────────────────────────────────────────────

@pytest.mark.asyncio
async def test_create_catalog_with_dot_in_name_returns_400(api_client):
    """Catalog names containing '.' are rejected with 400."""
    r = requests.post(f"{UC_BASE}/catalogs", json={"name": "cat.with.dots"}, timeout=10)
    assert r.status_code == 400, f"Expected 400 for name with dots, got {r.status_code}: {r.text}"


@pytest.mark.asyncio
async def test_create_catalog_with_slash_in_name_returns_400(api_client):
    """Catalog names containing '/' are rejected with 400."""
    r = requests.post(f"{UC_BASE}/catalogs", json={"name": "cat/slash"}, timeout=10)
    assert r.status_code == 400, f"Expected 400 for name with slash, got {r.status_code}: {r.text}"


@pytest.mark.asyncio
async def test_create_catalog_with_name_too_long_returns_400(api_client):
    """Catalog names exceeding 255 characters are rejected with 400."""
    long_name = "a" * 256
    r = requests.post(f"{UC_BASE}/catalogs", json={"name": long_name}, timeout=10)
    assert r.status_code == 400, f"Expected 400 for name too long, got {r.status_code}: {r.text}"


@pytest.mark.asyncio
async def test_create_catalog_with_space_in_name_returns_400(api_client):
    """Catalog names containing spaces are rejected with 400."""
    r = requests.post(f"{UC_BASE}/catalogs", json={"name": "cat with space"}, timeout=10)
    assert r.status_code == 400, f"Expected 400 for name with space, got {r.status_code}: {r.text}"


@pytest.mark.asyncio
async def test_create_schema_with_invalid_name_returns_400(api_client):
    """Schema names with dots are rejected with 400."""
    r = requests.post(f"{UC_BASE}/schemas",
                      json={"name": "schema.with.dot", "catalog_name": "unity"}, timeout=10)
    assert r.status_code == 400, f"Expected 400 for invalid schema name, got {r.status_code}: {r.text}"


@pytest.mark.asyncio
async def test_create_table_with_invalid_name_returns_400(api_client):
    """Table names with slashes are rejected with 400."""
    r = requests.post(f"{UC_BASE}/tables", json={
        "name": "table/slash", "catalog_name": "unity", "schema_name": "default",
        "table_type": "EXTERNAL", "data_source_format": "DELTA",
        "storage_location": "/tmp/gap_name", "columns": [],
    }, timeout=10)
    assert r.status_code == 400, f"Expected 400 for invalid table name, got {r.status_code}: {r.text}"


@pytest.mark.asyncio
async def test_create_volume_with_invalid_name_returns_400(api_client):
    """Volume names with dots are rejected with 400."""
    r = requests.post(f"{UC_BASE}/volumes", json={
        "name": "vol.dot", "catalog_name": "unity", "schema_name": "default",
        "volume_type": "EXTERNAL", "storage_location": "/tmp/gap_vol_name",
    }, timeout=10)
    assert r.status_code == 400, f"Expected 400 for invalid volume name, got {r.status_code}: {r.text}"


@pytest.mark.asyncio
async def test_create_catalog_with_empty_name_returns_400(api_client):
    """Empty catalog names are rejected with 400."""
    r = requests.post(f"{UC_BASE}/catalogs", json={"name": ""}, timeout=10)
    assert r.status_code == 400, f"Expected 400 for empty name, got {r.status_code}: {r.text}"
