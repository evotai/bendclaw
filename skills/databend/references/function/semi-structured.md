## Semi-structured Functions

Work with JSON, arrays, and maps (Variant type).

### JSON Parsing

| Function | Description |
|----------|-------------|
| `PARSE_JSON(s)` | Parse JSON string to Variant |
| `CHECK_JSON(s)` | Validate JSON format |
| `JSON_PARSE(s)` | Alias for PARSE_JSON |

### Access Values

| Function | Description |
|----------|-------------|
| `data:key` or `data['key']` | Access object field |
| `data[0]` | Access array element |
| `GET(data, 'key')` | Get field value |
| `GET_PATH(data, 'path')` | Get nested value with path |
| `JSON_EXTRACT_PATH_TEXT(data, path)` | Extract as text |

### Array Functions

| Function | Description |
|----------|-------------|
| `ARRAY_AGG(col)` | Aggregate into array |
| `ARRAY_SIZE(arr)`, `CARDINALITY(arr)` | Array length |
| `ARRAY_CONCAT(a, b)` | Concatenate arrays |
| `ARRAY_APPEND(arr, x)`, `ARRAY_PREPEND(arr, x)` | Add element |
| `ARRAY_FILTER(arr, f)`, `ARRAY_TRANSFORM(arr, f)` | Filter/transform |
| `UNNEST(arr)` | Flatten array to rows |

### Examples

```sql
-- Parse and access JSON
SELECT PARSE_JSON('{"name": "Alice", "age": 30}');
SELECT data:name, data['age'] FROM (SELECT PARSE_JSON('{"name":"Bob","age":25}') AS data);
SELECT GET_PATH(data, 'user.name') FROM table;

-- Array operations
SELECT ARRAY_SIZE([1, 2, 3]);                    → 3
SELECT ARRAY_CONCAT([1, 2], [3, 4]);             → [1, 2, 3, 4]
SELECT ARRAY_APPEND([1, 2], 3);                   → [1, 2, 3]
SELECT ARRAY_FILTER(x -> x > 1, [0, 1, 2, 3]);   → [2, 3]

-- Unnest array
SELECT * FROM UNNEST([1, 2, 3]) AS t(value);
```
