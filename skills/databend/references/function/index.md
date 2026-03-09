## Functions

Built-in functions for data transformation.

| Category | Description |
|----------|-------------|
| [String](string.md) | CONCAT, SUBSTRING, REPLACE, TRIM, regex |
| [DateTime](datetime.md) | NOW, DATE_ADD, DATE_DIFF, TO_DATE, EXTRACT |
| [Aggregate](aggregate.md) | SUM, COUNT, AVG, MIN, MAX, GROUP_CONCAT |
| [Window](window.md) | ROW_NUMBER, RANK, LAG, LEAD, NTILE |
| [Numeric](numeric.md) | ABS, ROUND, CEIL, FLOOR, POWER, SQRT |
| [Conditional](conditional.md) | CASE, COALESCE, NULLIF, IF, IFF |
| [Conversion](conversion.md) | CAST, TO_STRING, TO_INT, TO_FLOAT |
| [Semi-structured](semi-structured.md) | JSON parsing, array/map operations |
| [Bitmap](bitmap.md) | Bitmap operations for distinct counting |
| [Hash](hash.md) | MD5, SHA, XXHASH, SIPHASH |
| [UUID](uuid.md) | UUID generation |
| [IP Address](ip-address.md) | INET_ATON, INET_NTOA, IPv4 operations |
| [Context](context.md) | CURRENT_USER, DATABASE, VERSION |
| [System](system.md) | FUSE_SNAPSHOT, FUSE_BLOCK, clustering info |
| [Table](table-functions.md) | GENERATE_SERIES, FLATTEN, RESULT_SCAN |
| [Geospatial](geospatial.md) | ST_POINT, ST_DISTANCE, H3 functions |
| [Search](search.md) | MATCH, SCORE, QUERY for full-text search |
| [Vector](vector.md) | Vector similarity: cosine, L1, L2 distance |

Use `skill_read(path="databend/references/function/string.md")` to read details.
