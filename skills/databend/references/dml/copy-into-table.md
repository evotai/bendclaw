## COPY INTO TABLE

Load data from stage or external location into a table.

### Syntax

```sql
COPY INTO [<database>.]<table>
    FROM { internalStage | externalStage | externalLocation }
    [ PATTERN = '<regex>' ]
    FILE_FORMAT = ( TYPE = { PARQUET | CSV | TSV | NDJSON } [, ... ] )
    [ ON_ERROR = { continue | skip | abort } ]
    [ PURGE = true | false ]
```

### Examples

```sql
-- Load from internal stage
COPY INTO users
FROM @my_stage
FILE_FORMAT = (TYPE = PARQUET);

-- Load with pattern filter
COPY INTO users
FROM @my_stage
PATTERN = '.*[.]parquet'
FILE_FORMAT = (TYPE = PARQUET);

-- Load from S3
COPY INTO users
FROM 's3://bucket/path/'
CREDENTIALS = (AWS_KEY_ID = 'xxx', AWS_SECRET_KEY = 'xxx')
FILE_FORMAT = (TYPE = CSV, FIELD_DELIMITER = ',');

-- Load with transformation
COPY INTO users (id, name)
FROM (SELECT $1, $2 FROM @my_stage)
FILE_FORMAT = (TYPE = PARQUET);

-- Load CSV with options
COPY INTO users
FROM @my_stage
FILE_FORMAT = (TYPE = CSV, SKIP_HEADER = 1, FIELD_DELIMITER = '|');
```

### File Format Options

| Type | Key Options |
|------|-------------|
| PARQUET | `TYPE = PARQUET` |
| CSV | `SKIP_HEADER`, `FIELD_DELIMITER`, `RECORD_DELIMITER` |
| NDJSON | `TYPE = NDJSON` |

### Notes

- Supports internal/external stages and direct S3/GCS/Azure URLs
- `PURGE = true` deletes files after successful load
- `ON_ERROR` controls behavior on parse errors
