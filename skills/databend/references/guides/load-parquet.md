## Load Parquet

Load Parquet files into Databend tables.

### Overview

Parquet is the recommended format for Databend:
- Columnar storage for analytics
- Efficient compression
- Schema preserved

### Steps

#### 1. Create Target Table

```sql
CREATE TABLE users (
    id INT,
    name VARCHAR,
    email VARCHAR
);
```

#### 2. Create Stage (if loading from files)

```sql
-- Internal stage
CREATE STAGE my_stage;

-- Or external S3 stage
CREATE STAGE s3_stage
    URL = 's3://bucket/parquet/'
    CREDENTIALS = (AWS_KEY_ID = 'xxx', AWS_SECRET_KEY = 'xxx');
```

#### 3. Upload Files (for internal stage)

```sql
-- Generate test data to stage
COPY INTO @my_stage
FROM (SELECT number, 'User_' || number, 'user' || number || '@example.com' 
      FROM numbers(1000))
FILE_FORMAT = (TYPE = PARQUET);
```

#### 4. Load Data

```sql
-- Load all Parquet files
COPY INTO users
FROM @my_stage
PATTERN = '.*[.]parquet'
FILE_FORMAT = (TYPE = PARQUET);

-- Load with transformation
COPY INTO users (id, name)
FROM (SELECT $1, $2 FROM @my_stage)
FILE_FORMAT = (TYPE = PARQUET);

-- Load from S3 directly (no stage needed)
COPY INTO users
FROM 's3://bucket/path/'
CREDENTIALS = (AWS_KEY_ID = 'xxx', AWS_SECRET_KEY = 'xxx')
FILE_FORMAT = (TYPE = PARQUET);
```

### Options

```sql
COPY INTO users FROM @stage
    FILE_FORMAT = (TYPE = PARQUET)
    PATTERN = '.*[.]parquet'      -- filter files
    PURGE = true                  -- delete after load
    ON_ERROR = continue;          -- skip bad rows
```

### Notes

- Parquet schema auto-detected
- Column order matches file structure
- Use `$1, $2, ...` to reference columns by position
