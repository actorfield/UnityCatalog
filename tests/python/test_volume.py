import pytest
import os
import json

from unitycatalog.client import (
    CreateVolumeRequestContent,
    VolumeType,
)


@pytest.mark.asyncio
async def test_volume_list(volumes_api):
    api_response = await volumes_api.list_volumes("unity", "default")
    volume_names = {v.name for v in api_response.volumes}

    assert volume_names == {"txt_files", "json_files"}


@pytest.mark.asyncio
@pytest.mark.parametrize(
    "volume_name,volume_type",
    [
        ("txt_files", VolumeType.MANAGED),
        ("json_files", VolumeType.EXTERNAL),
    ],
)
async def test_volume_get(volumes_api, volume_name, volume_type):
    volume_info = await volumes_api.get_volume(f"unity.default.{volume_name}")

    assert volume_info.name == volume_name
    assert volume_info.catalog_name == "unity"
    assert volume_info.schema_name == "default"
    assert volume_info.volume_type == volume_type


@pytest.mark.asyncio
async def test_volume_create(volumes_api):
    storage_location = "/tmp/uc/myVolume"
    os.makedirs(storage_location, exist_ok=True)

    # Create a sample file in the volume location
    sample_file = os.path.join(storage_location, "c.json")
    with open(sample_file, "w") as f:
        json.dump({"marks": [95, 87, 76]}, f)

    volume_info = await volumes_api.create_volume(
        CreateVolumeRequestContent(
            name="myVolume",
            catalog_name="unity",
            schema_name="default",
            volume_type=VolumeType.EXTERNAL,
            storage_location=storage_location,
        )
    )

    try:
        assert volume_info.name == "myVolume"
        assert volume_info.catalog_name == "unity"
        assert volume_info.schema_name == "default"
        assert volume_info.volume_type == VolumeType.EXTERNAL
        assert volume_info.storage_location == f"file://{storage_location}"

        # Verify the volume is retrievable and its storage location is correct
        fetched = await volumes_api.get_volume("unity.default.myVolume")
        assert fetched.storage_location == f"file://{storage_location}"

        # Verify the file we placed in the volume location is accessible on disk
        assert os.path.exists(sample_file)
        with open(sample_file) as f:
            data = json.load(f)
        assert "marks" in data

    finally:
        await volumes_api.delete_volume("unity.default.myVolume")
        if os.path.exists(storage_location):
            import shutil
            shutil.rmtree(storage_location)
