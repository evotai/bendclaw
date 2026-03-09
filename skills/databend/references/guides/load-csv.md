## Load CSV

Load CSV files into Databend tables.

### Overview

CSV is a common format for data exchange. Databend provides flexible parsing options.

### Steps

#### 1. Create Target Table

```sql
CREATE TABLE users (
    id INT,
    name VARCHAR,
    email VARCHAR
);
```

#### 2. Load Data

```sql
-- Basic CSV load
COPY INTO users
FROM @my_stage
FILE_FORMAT = (TYPE = CSV);

-- With header row
COPY INTO users
FROM @my_stage
FILE_FORMAT = (TYPE = CSV, SKIP_HEADER = 1);

-- Custom delimiter
COPY INTO users
FROM @my_stage
FILE_FORMAT = (TYPE = CSV, FIELD_DELIMITER = '|');

-- From S3
COPY INTO users
FROM 's3://bucket/data.csv'
CREDENTIALS = (AWS_KEY_ID = 'xxx', AWS_SECRET_KEY = 'xxx')
FILE_FORMAT = (TYPE = CSV, SKIP_HEADER = 1);
```

### CSV Format Options

| Option | Description | Default |
|--------|-------------|---------|
| `TYPE` | Format type | - |
| `SKIP_HEADER` | Skip N header rows | 0 |
| `FIELD_DELIMITER` | Column separator | `,` |
| `RECORD_DELIMITER` | Row separator | `\n` |
| `QUOTE` | Quote character | `"` |
| `ESCAPE` | Escape character | `"` |
| `NULL_FIELD` | String for NULL | Empty |

### Examples

```sql
-- Tab-separated with header
COPY INTO users FROM @stage
FILE_FORMAT = (TYPE = CSV, SKIP_HEADER = 1, FIELD_DELIMITER = '\t');

-- Pipe-delimited
COPY INTO users FROM @stage
FILE_FORMAT = (TYPE = CSV, FIELD_DELIMITER = '|');

-- Handle NULL values
COPY INTO users FROM @stage
FILE_FORMAT = (TYPE = CSV, NULL_FIELD = 'NULL');

-- Custom quote character
COPY INTO users FROM @stage
FILE_FORMAT = (TYPE = CSV, QUOTE = "'", ESCAPE = "\\");
```

### Notes

- Columns map by position, not name
- Use transformation for column selection:
  ```sql
  COPY INTO users (id, name)
  FROM (SELECT $1, $2 FROM @stage)
  FILE_FORMAT = (TYPE = CSV);
  ```
