## Conversion Functions

Convert between data types.

### Functions

| Function | Description |
|----------|-------------|
| `CAST(x AS type)` | Type conversion |
| `TRY_CAST(x AS type)` | Returns NULL on failure |
| `TO_STRING(x)`, `TO_VARCHAR(x)` | Convert to string |
| `TO_INT(x)`, `TO_INT64(x)` | Convert to integer |
| `TO_FLOAT(x)`, `TO_FLOAT64(x)` | Convert to float |
| `TO_BOOLEAN(x)` | Convert to boolean |
| `TO_DECIMAL(x, p, s)` | Convert to decimal |
| `TO_DATE(x)`, `TO_DATETIME(x)` | Convert to date/timestamp |
| `TO_BINARY(x)` | Convert to binary |
| `HEX(x)`, `UNHEX(x)` | Hex encoding/decoding |

### Examples

```sql
-- CAST
SELECT CAST('123' AS INT);              → 123
SELECT CAST(123.45 AS VARCHAR);          → '123.45'
SELECT TRY_CAST('abc' AS INT);           → NULL (no error)

-- Type shortcuts
SELECT TO_STRING(123);                   → '123'
SELECT TO_INT('456');                    → 456
SELECT TO_FLOAT('3.14');                 → 3.14
SELECT TO_BOOLEAN('true');               → true
SELECT TO_DATE('2025-01-15');             → DATE

-- Hex encoding
SELECT HEX(255);                         → 'FF'
SELECT UNHEX('48656C6C6F');               → 'Hello' (binary)
```
