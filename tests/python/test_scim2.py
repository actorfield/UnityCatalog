"""
Integration tests for SCIM2 user management and auth token endpoints.
"""
import os
import pytest
import requests

UC_HOST = os.environ.get("UC_HOST", "http://localhost:8080")
UC_CTRL = f"{UC_HOST}/api/1.0/unity-control"
UC_BASE = f"{UC_HOST}/api/2.1/unity-catalog"


def ctrl_post(path, data):
    r = requests.post(f"{UC_CTRL}{path}", json=data, timeout=10)
    r.raise_for_status()
    return r.json()


def ctrl_get(path):
    r = requests.get(f"{UC_CTRL}{path}", timeout=10)
    r.raise_for_status()
    return r.json()


def ctrl_put(path, data):
    r = requests.put(f"{UC_CTRL}{path}", json=data, timeout=10)
    r.raise_for_status()
    return r.json()


def ctrl_delete(path):
    return requests.delete(f"{UC_CTRL}{path}", timeout=10)


# ── SCIM2 User CRUD ───────────────────────────────────────────────────────────

@pytest.mark.asyncio
async def test_scim2_user_create_and_get(api_client):
    """Create a SCIM2 user and retrieve by ID."""
    user = ctrl_post("/scim2/Users", {
        "userName": "scim_test_get@example.com",
        "active": True,
    })
    uid = user["id"]
    try:
        assert user["userName"] == "scim_test_get@example.com"
        assert user["active"] is True
        assert uid is not None

        fetched = ctrl_get(f"/scim2/Users/{uid}")
        assert fetched["id"] == uid
        assert fetched["userName"] == "scim_test_get@example.com"
    finally:
        ctrl_delete(f"/scim2/Users/{uid}")


@pytest.mark.asyncio
async def test_scim2_user_list(api_client):
    """Created user appears in list."""
    user = ctrl_post("/scim2/Users", {"userName": "scim_list@example.com", "active": True})
    uid = user["id"]
    try:
        result = ctrl_get("/scim2/Users")
        usernames = {u["userName"] for u in result["Resources"]}
        assert "scim_list@example.com" in usernames
        assert result["total_results"] >= 1
    finally:
        ctrl_delete(f"/scim2/Users/{uid}")


@pytest.mark.asyncio
async def test_scim2_user_update(api_client):
    """PUT /scim2/Users/:id updates the userName."""
    user = ctrl_post("/scim2/Users", {"userName": "scim_upd_old@example.com", "active": True})
    uid = user["id"]
    try:
        updated = ctrl_put(f"/scim2/Users/{uid}", {
            "userName": "scim_upd_new@example.com",
            "active": True,
        })
        assert updated["userName"] == "scim_upd_new@example.com"
        fetched = ctrl_get(f"/scim2/Users/{uid}")
        assert fetched["userName"] == "scim_upd_new@example.com"
    finally:
        ctrl_delete(f"/scim2/Users/{uid}")


@pytest.mark.asyncio
async def test_scim2_user_patch_disable(api_client):
    """PATCH /scim2/Users/:id can disable a user."""
    user = ctrl_post("/scim2/Users", {"userName": "scim_patch@example.com", "active": True})
    uid = user["id"]
    try:
        r = requests.patch(
            f"{UC_CTRL}/scim2/Users/{uid}",
            json={
                "schemas": ["urn:ietf:params:scim:api:messages:2.0:PatchOp"],
                "Operations": [{"op": "replace", "value": {"active": False}}],
            },
            timeout=10,
        )
        r.raise_for_status()
        assert r.json()["active"] is False
    finally:
        ctrl_delete(f"/scim2/Users/{uid}")


@pytest.mark.asyncio
async def test_scim2_user_delete(api_client):
    """DELETE /scim2/Users/:id returns 204 and user is gone."""
    user = ctrl_post("/scim2/Users", {"userName": "scim_del@example.com", "active": True})
    uid = user["id"]
    r = ctrl_delete(f"/scim2/Users/{uid}")
    assert r.status_code == 204
    r2 = requests.get(f"{UC_CTRL}/scim2/Users/{uid}", timeout=10)
    assert r2.status_code == 404


@pytest.mark.asyncio
async def test_scim2_get_me_no_auth_returns_anonymous(api_client):
    """GET /scim2/Me in --no-auth mode returns synthetic anonymous user."""
    r = requests.get(f"{UC_CTRL}/scim2/Me", timeout=10)
    assert r.status_code == 200
    body = r.json()
    assert "userName" in body


# ── Auth Tokens ───────────────────────────────────────────────────────────────

@pytest.mark.asyncio
async def test_auth_token_exchange_returns_bearer(api_client):
    """
    POST /auth/tokens with a valid subject_token issues a JWT bearer token.
    In --no-auth mode the server issues a token for any sub without DB lookup.
    """
    r = requests.post(
        f"{UC_CTRL}/auth/tokens",
        json={
            "grant_type": "urn:ietf:params:oauth:grant-type:token-exchange",
            "subject_token": "any_user@example.com",
            "subject_token_type": "urn:ietf:params:oauth:token-type:access_token",
        },
        timeout=10,
    )
    assert r.status_code == 200
    body = r.json()
    assert "access_token" in body
    assert body["token_type"] == "Bearer"
    assert body["issued_token_type"] == "urn:ietf:params:oauth:token-type:access_token"
    # Token should be a 3-part JWT
    assert len(body["access_token"].split(".")) == 3


@pytest.mark.asyncio
async def test_auth_token_wrong_grant_type_returns_400(api_client):
    """POST /auth/tokens with unsupported grant_type returns 400."""
    r = requests.post(
        f"{UC_CTRL}/auth/tokens",
        json={
            "grant_type": "password",
            "subject_token": "user@example.com",
            "subject_token_type": "urn:ietf:params:oauth:token-type:access_token",
        },
        timeout=10,
    )
    assert r.status_code == 400


@pytest.mark.asyncio
async def test_auth_logout_returns_200(api_client):
    """POST /auth/logout returns 200."""
    r = requests.post(f"{UC_CTRL}/auth/logout", json={}, timeout=10)
    assert r.status_code == 200


@pytest.mark.asyncio
async def test_jwks_endpoint_returns_keys(api_client):
    """GET /.well-known/jwks.json returns a JWKS with at least one key."""
    r = requests.get(f"{UC_HOST}/.well-known/jwks.json", timeout=10)
    assert r.status_code == 200
    body = r.json()
    assert "keys" in body
    assert len(body["keys"]) >= 1
    key = body["keys"][0]
    assert key["kty"] == "RSA"
    assert "kid" in key
