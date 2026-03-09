## Conditional Functions

Conditional logic and NULL handling.

### Functions

| Function | Description |
|----------|-------------|
| `CASE WHEN ... THEN ... END` | Conditional expression |
| `IF(cond, then, else)` | Simple if/else |
| `IFF(cond, then, else)` | Same as IF |
| `COALESCE(x, y, ...)` | First non-NULL value |
| `NULLIF(x, y)` | NULL if x equals y |
| `IFNULL(x, default)`, `NVL(x, default)` | Replace NULL |
| `NVL2(x, val_if_not_null, val_if_null)` | NULL check with two outcomes |
| `GREATEST(x, y, ...)`, `LEAST(x, y, ...)` | Max/min of values |
| `ISNULL(x)`, `ISNOTNULL(x)` | NULL check |

### Examples

```sql
-- CASE expression
SELECT CASE WHEN score >= 90 THEN 'A' WHEN score >= 80 THEN 'B' ELSE 'C' END;

-- IF function
SELECT IF(active, 'Yes', 'No');

-- NULL handling
SELECT COALESCE(phone, mobile, 'N/A');
SELECT NULLIF(status, 'pending');  -- NULL if status is 'pending'
SELECT IFNULL(deleted_at, 'active');

-- Greatest/Least
SELECT GREATEST(10, 20, 5), LEAST(10, 20, 5);  → 20, 5
```
