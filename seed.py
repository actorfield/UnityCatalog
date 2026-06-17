#!/usr/bin/env python3
"""Seed the Rust UC server with the same sample data as the Java server's PopulateTestDatabase."""
import requests, sys, json

BASE = sys.argv[1] if len(sys.argv) > 1 else "http://localhost:8080"
CAT  = f"{BASE}/api/2.1/unity-catalog"
CTRL = f"{BASE}/api/1.0/unity-control"

def post(url, data):
    r = requests.post(url, json=data, timeout=10)
    if r.status_code not in (200, 201):
        print(f"  WARN {url}: {r.status_code} {r.text[:200]}")
        return None
    return r.json()

def get(url):
    return requests.get(url, timeout=10).json()

print("Seeding unity catalog...")

# Catalog
post(f"{CAT}/catalogs", {"name":"unity","comment":"Main catalog"})

# Schema
post(f"{CAT}/schemas", {"name":"default","catalog_name":"unity","comment":"Default schema"})

# marksheet — MANAGED table
post(f"{CAT}/tables", {
    "name":"marksheet","catalog_name":"unity","schema_name":"default",
    "table_type":"MANAGED","data_source_format":"DELTA",
    "storage_location":"etc/data/managed/unity/default/tables/marksheet",
    "comment":"Managed table",
    "columns":[
        {"name":"id","type_text":"int","type_json":'{"name":"id","type":"integer","nullable":false,"metadata":{}}',
         "type_name":"INT","position":0,"nullable":False,"comment":"ID primary key"},
        {"name":"name","type_text":"string","type_json":'{"name":"name","type":"string","nullable":false,"metadata":{}}',
         "type_name":"STRING","position":1,"nullable":False,"comment":"Name of the entity"},
        {"name":"marks","type_text":"int","type_json":'{"name":"marks","type":"integer","nullable":true,"metadata":{}}',
         "type_name":"INT","position":2,"nullable":True,"comment":"Marks of the entity"},
    ]
})

# marksheet_uniform — EXTERNAL table (Delta + Iceberg Uniform)
post(f"{CAT}/tables", {
    "name":"marksheet_uniform","catalog_name":"unity","schema_name":"default",
    "table_type":"EXTERNAL","data_source_format":"DELTA",
    "storage_location":"etc/data/external/unity/default/tables/marksheet_uniform",
    "comment":"External table with Uniform Iceberg",
    "columns":[
        {"name":"id","type_text":"int","type_json":'{"name":"id","type":"integer","nullable":false,"metadata":{}}',
         "type_name":"INT","position":0,"nullable":False},
        {"name":"name","type_text":"string","type_json":'{"name":"name","type":"string","nullable":false,"metadata":{}}',
         "type_name":"STRING","position":1,"nullable":False},
        {"name":"marks","type_text":"int","type_json":'{"name":"marks","type":"integer","nullable":true,"metadata":{}}',
         "type_name":"INT","position":2,"nullable":True},
    ]
})

# numbers — EXTERNAL table
post(f"{CAT}/tables", {
    "name":"numbers","catalog_name":"unity","schema_name":"default",
    "table_type":"EXTERNAL","data_source_format":"DELTA",
    "storage_location":"etc/data/external/unity/default/tables/numbers",
    "comment":"External table",
    "columns":[
        {"name":"as_int","type_text":"int","type_json":'{"name":"as_int","type":"integer","nullable":false,"metadata":{}}',
         "type_name":"INT","position":0,"nullable":False},
        {"name":"as_double","type_text":"double","type_json":'{"name":"as_double","type":"double","nullable":false,"metadata":{}}',
         "type_name":"DOUBLE","position":1,"nullable":False},
    ]
})

# user_countries — EXTERNAL partitioned table
post(f"{CAT}/tables", {
    "name":"user_countries","catalog_name":"unity","schema_name":"default",
    "table_type":"EXTERNAL","data_source_format":"DELTA",
    "storage_location":"etc/data/external/unity/default/tables/user_countries",
    "comment":"Partitioned table",
    "columns":[
        {"name":"first_name","type_text":"string","type_json":'{"name":"first_name","type":"string","nullable":false,"metadata":{}}',
         "type_name":"STRING","position":0,"nullable":False},
        {"name":"age","type_text":"long","type_json":'{"name":"age","type":"long","nullable":false,"metadata":{}}',
         "type_name":"LONG","position":1,"nullable":False},
        {"name":"country","type_text":"string","type_json":'{"name":"country","type":"string","nullable":false,"metadata":{}}',
         "type_name":"STRING","position":2,"nullable":False,"partition_index":0},
    ]
})

print("  tables seeded")

# Volumes
post(f"{CAT}/volumes", {
    "name":"txt_files","catalog_name":"unity","schema_name":"default",
    "volume_type":"MANAGED","storage_location":"etc/data/managed/unity/default/volumes/txt_files",
    "comment":"Managed volume with txt files"
})
post(f"{CAT}/volumes", {
    "name":"json_files","catalog_name":"unity","schema_name":"default",
    "volume_type":"EXTERNAL","storage_location":"etc/data/external/unity/default/volumes/json_files",
    "comment":"External volume with json files"
})
print("  volumes seeded")

# Functions
post(f"{CAT}/functions", {"function_info":{
    "name":"sum","catalog_name":"unity","schema_name":"default",
    "comment":"Adds two numbers.",
    "data_type":"INT","full_data_type":"int",
    "external_language":"python","is_deterministic":True,"is_null_call":False,
    "parameter_style":"S","routine_body":"EXTERNAL",
    "routine_definition":"t = x + y + z\\nreturn t",
    "sql_data_access":"NO_SQL","security_type":"DEFINER","specific_name":"sum",
    "input_params":{"parameters":[
        {"name":"x","type_text":"int","type_name":"INT","type_json":'{"name":"x","type":"integer","nullable":false,"metadata":{}}',
         "position":0,"parameter_mode":"IN","parameter_type":"PARAM"},
        {"name":"y","type_text":"int","type_name":"INT","type_json":'{"name":"y","type":"integer","nullable":false,"metadata":{}}',
         "position":1,"parameter_mode":"IN","parameter_type":"PARAM"},
        {"name":"z","type_text":"int","type_name":"INT","type_json":'{"name":"z","type":"integer","nullable":false,"metadata":{}}',
         "position":2,"parameter_mode":"IN","parameter_type":"PARAM"},
    ]}
}})

post(f"{CAT}/functions", {"function_info":{
    "name":"lowercase","catalog_name":"unity","schema_name":"default",
    "comment":"Converts a string to lowercase.",
    "data_type":"STRING","full_data_type":"string",
    "external_language":"python","is_deterministic":True,"is_null_call":False,
    "parameter_style":"S","routine_body":"EXTERNAL",
    "routine_definition":"g = s.lower()\\nreturn g",
    "sql_data_access":"NO_SQL","security_type":"DEFINER","specific_name":"lowercase",
    "input_params":{"parameters":[
        {"name":"s","type_text":"string","type_name":"STRING","type_json":'{"name":"s","type":"string","nullable":false,"metadata":{}}',
         "position":0,"parameter_mode":"IN","parameter_type":"PARAM"},
    ]}
}})
print("  functions seeded")
print("Done. Seed data loaded successfully.")
