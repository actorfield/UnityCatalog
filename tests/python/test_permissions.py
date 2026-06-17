"""
Integration tests for GET/PATCH /permissions/:securable/:full_name.
Runs in --no-auth mode so AllowingAuthorizer is used —
tests focus on the API surface (parsing, response shape, error codes).
"""
import os
import pytest
import requests

UC_HOST = os.environ.get("UC_HOST", "http://localhost:8080")
UC_BASE = f"{UC_HOST}/api/2.1/unity-catalog"
UC_CTRL = f"{UC_HOST}/api/1.0/unity-control"


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


def delete(path, base=UC_BASE):
    return requests.delete(f"{base}{path}", timeout=10)


@pytest.mark.asyncio
async def test_permissions_get_catalog_returns_list(tables_api):
    """GET /permissions/catalog/<name> returns a PermissionsList with securable_type set."""
    result = get("/permissions/catalog/unity")
    assert "privilege_assignments" in result
    assert result["securable_type"] == "CATALOG"
    assert result["full_name"] == "unity"
    assert isinstance(result["privilege_assignments"], list)


@pytest.mark.asyncio
async def test_permissions_get_schema_returns_list(tables_api):
    """GET /permissions/schema/unity.default returns correct securable_type."""
    result = get("/permissions/schema/unity.default")
    assert result["securable_type"] == "SCHEMA"
    assert "privilege_assignments" in result


@pytest.mark.asyncio
async def test_permissions_get_table_returns_list(tables_api):
    """GET /permissions/table/unity.default.numbers returns table permissions."""
    result = get("/permissions/table/unity.default.numbers")
    assert result["securable_type"] == "TABLE"
    assert isinstance(result["privilege_assignments"], list)


@pytest.mark.asyncio
async def test_permissions_get_unknown_securable_returns_error(tables_api):
    """GET /permissions with unknown securable type returns 400."""
    r = requests.get(f"{UC_BASE}/permissions/spaceship/unity", timeout=10)
    assert r.status_code == 400
    assert "Unknown securable type" in r.json().get("message", "")


@pytest.mark.asyncio
async def test_permissions_get_missing_resource_returns_404(tables_api):
    """GET /permissions for non-existent resource returns 404."""
    r = requests.get(f"{UC_BASE}/permissions/catalog/nonexistent_cat_xyz", timeout=10)
    assert r.status_code == 404


@pytest.mark.asyncio
async def test_permissions_patch_grant_and_revoke(tables_api):
    """
    PATCH /permissions grants a privilege to a user then revokes it.
    In no-auth mode AllowingAuthorizer is used so the grant/revoke always succeeds.
    """
    # Create a test user to grant to
    user = post("/scim2/Users", {"userName": "perm_test@example.com", "active": True}, base=UC_CTRL)
    user_id = user["id"]
    try:
        # Grant USE_CATALOG on the unity catalog
        result = patch("/permissions/catalog/unity", {
            "changes": [{
                "principal": "perm_test@example.com",
                "add": ["USE_CATALOG"],
                "remove": [],
            }]
        })
        assert result["securable_type"] == "CATALOG"
        assert isinstance(result["privilege_assignments"], list)

        # Revoke it
        result = patch("/permissions/catalog/unity", {
            "changes": [{
                "principal": "perm_test@example.com",
                "add": [],
                "remove": ["USE_CATALOG"],
            }]
        })
        assert result["securable_type"] == "CATALOG"
    finally:
        delete(f"/scim2/Users/{user_id}", base=UC_CTRL)


@pytest.mark.asyncio
async def test_permissions_patch_unknown_privilege_returns_400(tables_api):
    """PATCH /permissions with an unknown privilege string returns 400."""
    # Create a user first
    user = post("/scim2/Users", {"userName": "perm_bad@example.com", "active": True}, base=UC_CTRL)
    try:
        r = requests.patch(f"{UC_BASE}/permissions/catalog/unity", json={
            "changes": [{
                "principal": "perm_bad@example.com",
                "add": ["INVALID_PRIVILEGE_XYZ"],
                "remove": [],
            }]
        }, timeout=10)
        assert r.status_code == 400, f"Expected 400, got {r.status_code}: {r.text}"
        assert "Unknown privilege" in r.json().get("message", "")
    finally:
        delete(f"/scim2/Users/{user['id']}", base=UC_CTRL)


@pytest.mark.asyncio
async def test_permissions_patch_unknown_user_returns_404(tables_api):
    """PATCH /permissions with unknown principal email returns 404."""
    r = requests.patch(f"{UC_BASE}/permissions/catalog/unity", json={
        "changes": [{
            "principal": "nobody_exists_xyz@example.com",
            "add": ["USE_CATALOG"],
            "remove": [],
        }]
    }, timeout=10)
    assert r.status_code == 404
