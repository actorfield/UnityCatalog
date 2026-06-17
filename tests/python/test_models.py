"""
Integration tests for registered models + model versions full lifecycle.
"""
import os
import pytest
import requests

from unitycatalog.client import (
    CreateRegisteredModel,
    CreateModelVersion,
    RegisteredModelsApi,
    ModelVersionsApi,
)

UC_HOST = os.environ.get("UC_HOST", "http://localhost:8080")
UC_BASE = f"{UC_HOST}/api/2.1/unity-catalog"


def api_delete(path):
    return requests.delete(f"{UC_BASE}{path}", timeout=10)


# ── Registered Model CRUD ─────────────────────────────────────────────────────

@pytest.mark.asyncio
async def test_model_create_and_get(api_client):
    """Create a registered model and verify it is retrievable."""
    api = RegisteredModelsApi(api_client)
    model = await api.create_registered_model(CreateRegisteredModel(
        name="test_model_get",
        catalog_name="unity",
        schema_name="default",
        comment="Test model",
    ))
    try:
        assert model.name == "test_model_get"
        assert model.catalog_name == "unity"
        assert model.schema_name == "default"
        assert model.comment == "Test model"
        assert model.id is not None

        fetched = await api.get_registered_model("unity.default.test_model_get")
        assert fetched.name == "test_model_get"
        assert fetched.id == model.id
    finally:
        api_delete("/models/unity.default.test_model_get")


@pytest.mark.asyncio
async def test_model_list(api_client):
    """Created model appears in list."""
    api = RegisteredModelsApi(api_client)
    await api.create_registered_model(CreateRegisteredModel(
        name="test_model_list",
        catalog_name="unity",
        schema_name="default",
    ))
    try:
        result = await api.list_registered_models(catalog_name="unity", schema_name="default")
        names = {m.name for m in result.registered_models}
        assert "test_model_list" in names
    finally:
        api_delete("/models/unity.default.test_model_list")


@pytest.mark.asyncio
async def test_model_update(api_client):
    """PATCH registered model updates comment."""
    api = RegisteredModelsApi(api_client)
    await api.create_registered_model(CreateRegisteredModel(
        name="test_model_upd",
        catalog_name="unity",
        schema_name="default",
        comment="original",
    ))
    try:
        await api.update_registered_model("unity.default.test_model_upd",
            update_registered_model={"comment": "updated comment"})
        fetched = await api.get_registered_model("unity.default.test_model_upd")
        assert fetched.comment == "updated comment"
    finally:
        api_delete("/models/unity.default.test_model_upd")


@pytest.mark.asyncio
async def test_model_delete(api_client):
    """Deleted model returns 404."""
    api = RegisteredModelsApi(api_client)
    await api.create_registered_model(CreateRegisteredModel(
        name="test_model_del",
        catalog_name="unity",
        schema_name="default",
    ))
    await api.delete_registered_model("unity.default.test_model_del")
    r = requests.get(f"{UC_BASE}/models/unity.default.test_model_del", timeout=10)
    assert r.status_code == 404


# ── Model Version lifecycle ───────────────────────────────────────────────────

@pytest.mark.asyncio
async def test_model_version_create_and_get(api_client):
    """Create a model version and verify status is PENDING_REGISTRATION."""
    models_api = RegisteredModelsApi(api_client)
    versions_api = ModelVersionsApi(api_client)

    await models_api.create_registered_model(CreateRegisteredModel(
        name="test_ver_model",
        catalog_name="unity",
        schema_name="default",
    ))
    try:
        ver = await versions_api.create_model_version(CreateModelVersion(
            model_name="test_ver_model",
            catalog_name="unity",
            schema_name="default",
            source="s3://ml/run1/artifacts",
            run_id="run_001",
            comment="First version",
        ))
        assert ver.version == 1
        assert ver.status.value == "PENDING_REGISTRATION"
        assert ver.run_id == "run_001"
        assert ver.source == "s3://ml/run1/artifacts"

        fetched = await versions_api.get_model_version("unity.default.test_ver_model", 1)
        assert fetched.version == 1
        assert fetched.comment == "First version"
    finally:
        api_delete("/models/unity.default.test_ver_model/versions/1")
        api_delete("/models/unity.default.test_ver_model")


@pytest.mark.asyncio
async def test_model_version_list(api_client):
    """Created versions appear in list."""
    models_api = RegisteredModelsApi(api_client)
    versions_api = ModelVersionsApi(api_client)

    await models_api.create_registered_model(CreateRegisteredModel(
        name="test_ver_list_model",
        catalog_name="unity",
        schema_name="default",
    ))
    try:
        await versions_api.create_model_version(CreateModelVersion(
            model_name="test_ver_list_model",
            catalog_name="unity",
            schema_name="default",
            source="s3://ml/run1",
        ))
        await versions_api.create_model_version(CreateModelVersion(
            model_name="test_ver_list_model",
            catalog_name="unity",
            schema_name="default",
            source="s3://ml/run2",
        ))
        result = await versions_api.list_model_versions("unity.default.test_ver_list_model")
        assert len(result.model_versions) == 2
        versions = sorted([v.version for v in result.model_versions])
        assert versions == [1, 2]
    finally:
        api_delete("/models/unity.default.test_ver_list_model/versions/2")
        api_delete("/models/unity.default.test_ver_list_model/versions/1")
        api_delete("/models/unity.default.test_ver_list_model")


@pytest.mark.asyncio
async def test_model_version_finalize(api_client):
    """Finalizing a model version sets status to READY."""
    models_api = RegisteredModelsApi(api_client)
    versions_api = ModelVersionsApi(api_client)

    await models_api.create_registered_model(CreateRegisteredModel(
        name="test_ver_finalize",
        catalog_name="unity",
        schema_name="default",
    ))
    try:
        await versions_api.create_model_version(CreateModelVersion(
            model_name="test_ver_finalize",
            catalog_name="unity",
            schema_name="default",
            source="s3://ml/run1",
        ))

        r = requests.patch(
            f"{UC_BASE}/models/unity.default.test_ver_finalize/versions/1/finalize",
            json={"status": "READY"}, timeout=10,
        )
        r.raise_for_status()
        assert r.json()["status"] == "READY"

        fetched = await versions_api.get_model_version("unity.default.test_ver_finalize", 1)
        assert fetched.status.value == "READY"
    finally:
        api_delete("/models/unity.default.test_ver_finalize/versions/1")
        api_delete("/models/unity.default.test_ver_finalize")


@pytest.mark.asyncio
async def test_model_version_update_comment(api_client):
    """PATCH model version persists comment."""
    models_api = RegisteredModelsApi(api_client)
    versions_api = ModelVersionsApi(api_client)

    await models_api.create_registered_model(CreateRegisteredModel(
        name="test_ver_upd_comment",
        catalog_name="unity",
        schema_name="default",
    ))
    try:
        await versions_api.create_model_version(CreateModelVersion(
            model_name="test_ver_upd_comment",
            catalog_name="unity",
            schema_name="default",
            source="s3://ml/run1",
            comment="original",
        ))
        await versions_api.update_model_version(
            "unity.default.test_ver_upd_comment", 1,
            update_model_version={"comment": "updated"},
        )
        fetched = await versions_api.get_model_version("unity.default.test_ver_upd_comment", 1)
        assert fetched.comment == "updated"
    finally:
        api_delete("/models/unity.default.test_ver_upd_comment/versions/1")
        api_delete("/models/unity.default.test_ver_upd_comment")


@pytest.mark.asyncio
async def test_model_version_delete(api_client):
    """Deleted version returns 404."""
    models_api = RegisteredModelsApi(api_client)
    versions_api = ModelVersionsApi(api_client)

    await models_api.create_registered_model(CreateRegisteredModel(
        name="test_ver_del_model",
        catalog_name="unity",
        schema_name="default",
    ))
    try:
        await versions_api.create_model_version(CreateModelVersion(
            model_name="test_ver_del_model",
            catalog_name="unity",
            schema_name="default",
            source="s3://ml/run1",
        ))
        await versions_api.delete_model_version("unity.default.test_ver_del_model", 1)
        r = requests.get(
            f"{UC_BASE}/models/unity.default.test_ver_del_model/versions/1",
            timeout=10,
        )
        assert r.status_code == 404
    finally:
        api_delete("/models/unity.default.test_ver_del_model")
