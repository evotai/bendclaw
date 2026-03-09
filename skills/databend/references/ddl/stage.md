## STAGE

File storage for data loading and unloading.

### Commands

```sql
CREATE [ OR REPLACE ] STAGE [ IF NOT EXISTS ] <name>
    [ URL = '<location>' ]
    [ CREDENTIALS = ( <key> = '<value>' [ , ... ] ) ]
    [ FILE_FORMAT = ( TYPE = { PARQUET | CSV | NDJSON } [ , ... ] ) ]

DROP STAGE [ IF EXISTS ] <name>
SHOW STAGES
DESCRIBE STAGE <name>
```

### Stage Types

| Type | URL Format | Description |
|------|------------|-------------|
| Internal | (no URL) | Databend-managed storage |
| S3 | `s3://bucket/path/` | Amazon S3 |
| GCS | `gcs://bucket/path/` | Google Cloud Storage |
| Azure | `azure://container/path/` | Azure Blob Storage |
| HTTP | `https://url/` | HTTP endpoint |

### Examples

```sql
-- Internal stage
CREATE STAGE my_stage;

-- S3 stage with credentials
CREATE STAGE s3_stage
    URL = 's3://my-bucket/data/'
    CREDENTIALS = (AWS_KEY_ID = 'xxx', AWS_SECRET_KEY = 'xxx');

-- GCS stage
CREATE STAGE gcs_stage URL = 'gcs://my-bucket/data/';

-- With default format
CREATE STAGE csv_stage
    URL = 's3://bucket/csv/'
    FILE_FORMAT = (TYPE = CSV, SKIP_HEADER = 1);

-- List files
LIST @my_stage;
LIST @my_stage/pattern/;

-- Remove stage
DROP STAGE my_stage;
```

### Notes

- Internal stages: Databend-managed, no credentials needed
- External stages: require credentials for private buckets
- `FILE_FORMAT` sets default for COPY operations
