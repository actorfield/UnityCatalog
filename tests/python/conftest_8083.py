"""
Pytest config for integration tests against the Rust Unity Catalog server.
Expects the server to already be running on http://localhost:8083 (--no-auth).
Start with:
  ./target/debug/uc-server --port 8080 --config-dir /tmp/uc/etc/conf \
    --database-url "sqlite:/tmp/uc/etc/db/uc.db?mode=rwc" --no-auth
Then seed:
  python3 seed.py
"""
import pytest
import pytest_asyncio
from unitycatalog.client import (
    ApiClient, CatalogsApi, Configuration, FunctionsApi,
    SchemasApi, TablesApi, VolumesApi,
)

UC_HOST = "http://localhost:8083/api/2.1/unity-catalog"


@pytest.fixture(scope="session", autouse=True)
def uc_server():
    """Server is expected to already be running."""
    yield


@pytest_asyncio.fixture()
async def api_client():
    config = Configuration(host=UC_HOST)
    client = ApiClient(config)
    yield client
    await client.close()


@pytest_asyncio.fixture()
async def catalogs_api(api_client): return CatalogsApi(api_client)
@pytest_asyncio.fixture()
async def schemas_api(api_client): return SchemasApi(api_client)
@pytest_asyncio.fixture()
async def tables_api(api_client): return TablesApi(api_client)
@pytest_asyncio.fixture()
async def volumes_api(api_client): return VolumesApi(api_client)
@pytest_asyncio.fixture()
async def functions_api(api_client): return FunctionsApi(api_client)
