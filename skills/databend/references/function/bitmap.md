## Bitmap Functions

Efficient distinct counting and set operations on bitmaps.

### Functions

| Function | Description |
|----------|-------------|
| `BUILD_BITMAP(array)` | Create bitmap from array |
| `BITMAP_CARDINALITY(bm)` | Count bits set |
| `BITMAP_AND(bm1, bm2)`, `BITMAP_OR(bm1, bm2)` | Set operations |
| `BITMAP_XOR(bm1, bm2)` | XOR operation |
| `BITMAP_CONTAINS(bm, x)` | Check if bit is set |
| `BITMAP_TO_ARRAY(bm)` | Convert to integer array |
| `BITMAP_MIN(bm)`, `BITMAP_MAX(bm)` | Min/max bit position |
| `INTERSECT_COUNT(bm1, bm2)` | Count of intersection |

### Examples

```sql
-- Create and query bitmap
SELECT BUILD_BITMAP([1, 2, 3, 5, 7]);
SELECT BITMAP_CARDINALITY(BUILD_BITMAP([1, 2, 3]));  → 3

-- Set operations
SELECT BITMAP_AND(BUILD_BITMAP([1, 2, 3]), BUILD_BITMAP([2, 3, 4]));
SELECT BITMAP_OR(BUILD_BITMAP([1, 2]), BUILD_BITMAP([3, 4]));

-- Check membership
SELECT BITMAP_CONTAINS(BUILD_BITMAP([1, 5, 10]), 5);  → true

-- Convert
SELECT BITMAP_TO_ARRAY(BUILD_BITMAP([1, 3, 5]));      → [1, 3, 5]
```

### Use Case

Efficient for distinct count in pre-aggregated tables:
```sql
-- Store bitmap for distinct user_ids per day
SELECT date, BUILD_BITMAP(ARRAY_AGG(user_id)) AS user_bitmap
FROM events GROUP BY date;

-- Query distinct count
SELECT BITMAP_CARDINALITY(user_bitmap) FROM daily_users;
```
