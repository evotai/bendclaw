## FILE FORMAT

Reusable file format definitions.

### Commands

```sql
CREATE [ OR REPLACE ] FILE FORMAT [ IF NOT EXISTS ] <name>
    TYPE = { CSV | PARQUET | NDJSON | AVRO | ORC }
    [ <format_option> = <value> [ , ... ] ]

DROP FILE FORMAT <name>
SHOW FILE FORMATS
DESCRIBE FILE FORMAT <name>
```

### CSV Options

| Option | Description | Default |
|--------|-------------|---------|
| `FIELD_DELIMITER` | Column separator | `,` |
| `RECORD_DELIMITER` | Row separator | `\n` |
| `SKIP_HEADER` | Skip N header rows | 0 |
| `QUOTE` | Quote character | `"` |
| `ESCAPE` | Escape character | `"` |
| `NULL_FIELD` | NULL representation | (empty) |

### Examples

```sql
-- CSV format
CREATE FILE FORMAT my_csv
    TYPE = CSV
    FIELD_DELIMITER = '|'
    SKIP_HEADER = 1;

-- Parquet format
CREATE FILE FORMAT my_parquet TYPE = PARQUET;

-- Use in COPY
COPY INTO users FROM @stage
    FILE_FORMAT = (FORMAT_NAME = 'my_csv');

-- Use in stage
CREATE STAGE csv_stage
    URL = 's3://bucket/'
    FILE_FORMAT = (FORMAT_NAME = 'my_csv');
```

### Notes

- Reusable across stages and COPY commands
- Simplifies format specification
