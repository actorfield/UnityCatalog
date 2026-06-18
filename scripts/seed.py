#!/usr/bin/env python3
"""Seed the Rust UC server with sample data.

Usage:
    UC_HOST=http://localhost:8080 UC_TOKEN=<token> python3 seed.py

UC_TOKEN is the admin token from /etc/uc/conf/token.txt on the conf PVC.
See manifests/NOTES.md checkpoint 12 for how to retrieve it.
"""
import os, requests, sys, json

BASE  = sys.argv[1] if len(sys.argv) > 1 else os.environ.get("UC_HOST", "http://localhost:8080")
TOKEN = os.environ.get("UC_TOKEN", "")
CAT   = f"{BASE}/api/2.1/unity-catalog"
CTRL  = f"{BASE}/api/1.0/unity-control"

# S3 storage root — Garage bucket for managed tables/volumes
STORAGE_ROOT = os.environ.get("UC_STORAGE_ROOT", "s3://uc-storage")

if not TOKEN:
    print("ERROR: UC_TOKEN env var required. See manifests/NOTES.md checkpoint 12.")
    sys.exit(1)

HEADERS = {"Authorization": f"Bearer {TOKEN}", "Content-Type": "application/json"}

def post(url, data):
    r = requests.post(url, json=data, headers=HEADERS, timeout=10)
    if r.status_code not in (200, 201):
        print(f"  WARN {url}: {r.status_code} {r.text[:200]}")
        return None
    return r.json()

def get(url):
    return requests.get(url, headers=HEADERS, timeout=10).json()

print(f"Seeding Unity Catalog at {BASE} ...")
print(f"Storage root: {STORAGE_ROOT}")

# Catalog — storage_root points to Garage so managed tables resolve to s3://
post(f"{CAT}/catalogs", {
    "name": "unity",
    "comment": "Main catalog",
    "storage_root": STORAGE_ROOT,
})

# Schema
post(f"{CAT}/schemas", {"name": "default", "catalog_name": "unity", "comment": "Default schema"})

# marksheet — MANAGED table (staging table flow — UC allocates path under storage_root)
_staging = post(f"{CAT}/staging-tables", {"name": "marksheet", "catalog_name": "unity", "schema_name": "default"})
if _staging:
    post(f"{CAT}/tables", {
        "name": "marksheet", "catalog_name": "unity", "schema_name": "default",
        "table_type": "MANAGED", "data_source_format": "DELTA",
        "storage_location": _staging["staging_location"],
        "comment": "Managed Delta table — data in Garage S3",
        "columns": [
            {"name": "id",    "type_text": "int",    "type_json": '{"name":"id","type":"integer","nullable":false,"metadata":{}}',    "type_name": "INT",    "position": 0, "nullable": False, "comment": "ID primary key"},
            {"name": "name",  "type_text": "string", "type_json": '{"name":"name","type":"string","nullable":false,"metadata":{}}',   "type_name": "STRING", "position": 1, "nullable": False, "comment": "Name"},
            {"name": "marks", "type_text": "int",    "type_json": '{"name":"marks","type":"integer","nullable":true,"metadata":{}}',  "type_name": "INT",    "position": 2, "nullable": True,  "comment": "Marks"},
        ]
    })

# numbers — EXTERNAL table pointing to Garage
post(f"{CAT}/tables", {
    "name": "numbers", "catalog_name": "unity", "schema_name": "default",
    "table_type": "EXTERNAL", "data_source_format": "DELTA",
    "storage_location": f"{STORAGE_ROOT}/external/unity/default/tables/numbers",
    "comment": "External Delta table in Garage",
    "columns": [
        {"name": "as_int",    "type_text": "int",    "type_json": '{"name":"as_int","type":"integer","nullable":false,"metadata":{}}',  "type_name": "INT",    "position": 0, "nullable": False},
        {"name": "as_double", "type_text": "double", "type_json": '{"name":"as_double","type":"double","nullable":false,"metadata":{}}', "type_name": "DOUBLE", "position": 1, "nullable": False},
    ]
})
print("  tables seeded")

# Volumes — managed volume in Garage
post(f"{CAT}/volumes", {
    "name": "txt_files", "catalog_name": "unity", "schema_name": "default",
    "volume_type": "MANAGED",
    "comment": "Managed volume — data in Garage S3"
})
post(f"{CAT}/volumes", {
    "name": "json_files", "catalog_name": "unity", "schema_name": "default",
    "volume_type": "EXTERNAL",
    "storage_location": f"{STORAGE_ROOT}/external/unity/default/volumes/json_files",
    "comment": "External volume in Garage"
})
print("  volumes seeded")

# Functions
post(f"{CAT}/functions", {"function_info": {
    "name": "sum", "catalog_name": "unity", "schema_name": "default",
    "comment": "Adds two numbers.",
    "data_type": "INT", "full_data_type": "int",
    "external_language": "python", "is_deterministic": True, "is_null_call": False,
    "parameter_style": "S", "routine_body": "EXTERNAL",
    "routine_definition": "t = x + y + z\nreturn t",
    "sql_data_access": "NO_SQL", "security_type": "DEFINER", "specific_name": "sum",
    "input_params": {"parameters": [
        {"name": "x", "type_text": "int", "type_name": "INT", "type_json": '{"name":"x","type":"integer","nullable":false,"metadata":{}}', "position": 0, "parameter_mode": "IN", "parameter_type": "PARAM"},
        {"name": "y", "type_text": "int", "type_name": "INT", "type_json": '{"name":"y","type":"integer","nullable":false,"metadata":{}}', "position": 1, "parameter_mode": "IN", "parameter_type": "PARAM"},
        {"name": "z", "type_text": "int", "type_name": "INT", "type_json": '{"name":"z","type":"integer","nullable":false,"metadata":{}}', "position": 2, "parameter_mode": "IN", "parameter_type": "PARAM"},
    ]}
}})
print("  functions seeded")

print("\nDone. Verify with:")
print(f"  curl -H 'Authorization: Bearer $UC_TOKEN' {CAT}/catalogs")
