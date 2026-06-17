"""
Integration tests for credentials and external locations CRUD.
"""
import os
import pytest
import requests

from unitycatalog.client import Configuration, ApiClient

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


def patch(path, data):
    r = requests.patch(f"{UC_BASE}{path}", json=data, timeout=10)
    r.raise_for_status()
    return r.json()


def delete(path):
    return requests.delete(f"{UC_BASE}{path}", timeout=10)


# ── Credentials CRUD ──────────────────────────────────────────────────────────

@pytest.mark.asyncio
async def test_credential_create_and_get(tables_api):
    """Create an AWS IAM role credential and verify it is retrievable."""
    cred = post("/credentials", {
        "name": "test_cred_get",
        "purpose": "AWS_IAM_ROLE",
        "aws_iam_role": {"role_arn": "arn:aws:iam::123456789:role/test-role"},
        "comment": "Test credential",
    })
    try:
        assert cred["name"] == "test_cred_get"
        assert cred["purpose"] == "AWS_IAM_ROLE"
        assert "id" in cred

        fetched = get("/credentials/test_cred_get")
        assert fetched["name"] == "test_cred_get"
        assert fetched["id"] == cred["id"]
        assert fetched["aws_iam_role"]["role_arn"] == "arn:aws:iam::123456789:role/test-role"
    finally:
        delete("/credentials/test_cred_get")


@pytest.mark.asyncio
async def test_credential_list(tables_api):
    """Created credentials appear in list response."""
    post("/credentials", {
        "name": "test_cred_list",
        "purpose": "AWS_IAM_ROLE",
        "aws_iam_role": {"role_arn": "arn:aws:iam::123:role/r"},
    })
    try:
        result = get("/credentials")
        names = {c["name"] for c in result["credentials"]}
        assert "test_cred_list" in names
    finally:
        delete("/credentials/test_cred_list")


@pytest.mark.asyncio
async def test_credential_update(tables_api):
    """PATCH credential updates comment."""
    post("/credentials", {
        "name": "test_cred_update",
        "purpose": "AWS_IAM_ROLE",
        "aws_iam_role": {"role_arn": "arn:aws:iam::123:role/r"},
    })
    try:
        updated = patch("/credentials/test_cred_update", {"comment": "updated comment"})
        assert updated["name"] == "test_cred_update"
        fetched = get("/credentials/test_cred_update")
        assert fetched["comment"] == "updated comment"
    finally:
        delete("/credentials/test_cred_update")


@pytest.mark.asyncio
async def test_credential_delete(tables_api):
    """Deleted credential returns 404 on subsequent GET."""
    post("/credentials", {
        "name": "test_cred_delete",
        "purpose": "AWS_IAM_ROLE",
        "aws_iam_role": {"role_arn": "arn:aws:iam::123:role/r"},
    })
    delete("/credentials/test_cred_delete")
    r = requests.get(f"{UC_BASE}/credentials/test_cred_delete", timeout=10)
    assert r.status_code == 404


@pytest.mark.asyncio
async def test_credential_duplicate_name_rejected(tables_api):
    """Creating two credentials with the same name returns 400."""
    post("/credentials", {
        "name": "test_cred_dup",
        "purpose": "AWS_IAM_ROLE",
        "aws_iam_role": {"role_arn": "arn:aws:iam::123:role/r"},
    })
    try:
        r = requests.post(f"{UC_BASE}/credentials", json={
            "name": "test_cred_dup",
            "purpose": "AWS_IAM_ROLE",
            "aws_iam_role": {"role_arn": "arn:aws:iam::123:role/r"},
        }, timeout=10)
        assert r.status_code in (400, 409), f"Expected 400/409, got {r.status_code}"
    finally:
        delete("/credentials/test_cred_dup")


# ── External Locations CRUD ───────────────────────────────────────────────────

@pytest.fixture(autouse=False)
def ext_loc_cred():
    """Helper credential for external location tests."""
    post("/credentials", {
        "name": "ext_loc_test_cred",
        "purpose": "AWS_IAM_ROLE",
        "aws_iam_role": {"role_arn": "arn:aws:iam::123:role/ext"},
    })
    yield "ext_loc_test_cred"
    delete("/credentials/ext_loc_test_cred")


@pytest.mark.asyncio
async def test_external_location_create_and_get(ext_loc_cred):
    """Create external location and verify it is retrievable."""
    el = post("/external-locations", {
        "name": "test_ext_loc",
        "url": "s3://my-bucket/path",
        "credential_name": ext_loc_cred,
        "comment": "Test external location",
    })
    try:
        assert el["name"] == "test_ext_loc"
        assert el["url"] == "s3://my-bucket/path"
        assert "id" in el

        fetched = get("/external-locations/test_ext_loc")
        assert fetched["name"] == "test_ext_loc"
        assert fetched["id"] == el["id"]
    finally:
        delete("/external-locations/test_ext_loc")


@pytest.mark.asyncio
async def test_external_location_list(ext_loc_cred):
    """Created external location appears in list."""
    post("/external-locations", {
        "name": "test_ext_loc_list",
        "url": "s3://bucket/list-path",
        "credential_name": ext_loc_cred,
    })
    try:
        result = get("/external-locations")
        names = {e["name"] for e in result["external_locations"]}
        assert "test_ext_loc_list" in names
    finally:
        delete("/external-locations/test_ext_loc_list")


@pytest.mark.asyncio
async def test_external_location_update(ext_loc_cred):
    """PATCH external location updates comment."""
    post("/external-locations", {
        "name": "test_ext_loc_upd",
        "url": "s3://bucket/upd",
        "credential_name": ext_loc_cred,
    })
    try:
        patch("/external-locations/test_ext_loc_upd", {"comment": "updated"})
        fetched = get("/external-locations/test_ext_loc_upd")
        assert fetched["comment"] == "updated"
    finally:
        delete("/external-locations/test_ext_loc_upd")


@pytest.mark.asyncio
async def test_external_location_delete(ext_loc_cred):
    """Deleted external location returns 404."""
    post("/external-locations", {
        "name": "test_ext_loc_del",
        "url": "s3://bucket/del",
        "credential_name": ext_loc_cred,
    })
    delete("/external-locations/test_ext_loc_del")
    r = requests.get(f"{UC_BASE}/external-locations/test_ext_loc_del", timeout=10)
    assert r.status_code == 404
