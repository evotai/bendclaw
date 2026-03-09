## Table Functions

Return tables (used in FROM clause).

### Functions

| Function | Description |
|----------|-------------|
| `GENERATE_SERIES(start, end, step)` | Number sequence |
| `NUMBERS(n)` | Numbers 0 to n-1 |
| `FLATTEN(input)` | Flatten array/object to rows |
| `RESULT_SCAN(query_id)` | Reuse previous result |
| `LIST_STAGE(stage)` | List stage files |
| `INFER_SCHEMA(location)` | Detect file schema |
| `READ_FILE(location)` | Read file contents |
| `ICEBERG_SNAPSHOT(table)` | Iceberg snapshot info |
| `STREAM_STATUS(stream)` | Stream consumption status |

### Examples

```sql
-- Generate numbers
SELECT * FROM GENERATE_SERIES(1, 100, 2);  -- 1, 3, 5, ..., 99
SELECT * FROM NUMBERS(10);                  -- 0, 1, 2, ..., 9

-- Flatten array
SELECT * FROM FLATTEN(INPUT => [1, 2, 3]);  -- rows: 1, 2, 3

-- Reuse query result
SELECT * FROM RESULT_SCAN(LAST_QUERY_ID());

-- List stage files
SELECT * FROM LIST_STAGE('@my_stage');

-- Detect schema
SELECT * FROM INFER_SCHEMA(location => '@my_stage/data.parquet');
```
