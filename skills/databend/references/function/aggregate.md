## Aggregate Functions

Functions that compute a single result from multiple rows.

### Common Functions

| Function | Description |
|----------|-------------|
| `COUNT(*)`, `COUNT(col)` | Count rows or non-NULL values |
| `COUNT(DISTINCT col)` | Count unique values |
| `SUM(col)` | Sum of values |
| `AVG(col)` | Average value |
| `MIN(col)`, `MAX(col)` | Minimum/Maximum |
| `GROUP_CONCAT(col, sep)` | Concatenate values |
| `STDDEV(col)`, `VARIANCE(col)` | Statistical |

### Examples

```sql
-- Basic aggregates
SELECT COUNT(*) AS total FROM orders;                    → row count
SELECT COUNT(DISTINCT user_id) FROM orders;              → unique users
SELECT SUM(total), AVG(total) FROM orders;                → sum and average
SELECT MIN(created_at), MAX(created_at) FROM orders;      → date range

-- With GROUP BY
SELECT category, COUNT(*) AS count, SUM(price) AS revenue
FROM products
GROUP BY category;

-- With HAVING (filter after aggregation)
SELECT category, COUNT(*) AS count
FROM products
GROUP BY category
HAVING COUNT(*) > 10;

-- Conditional aggregation
SELECT 
    COUNT(*) AS total,
    COUNT(*) FILTER (WHERE status = 'completed') AS completed,
    SUM(total) FILTER (WHERE status = 'completed') AS completed_revenue
FROM orders;

-- Multiple aggregates
SELECT 
    user_id,
    COUNT(*) AS order_count,
    SUM(total) AS total_spent,
    AVG(total) AS avg_order,
    MAX(created_at) AS last_order
FROM orders
GROUP BY user_id;

-- List aggregation
SELECT GROUP_CONCAT(name, ', ') FROM users;              → 'Alice, Bob, Charlie'
```

### Notes

- `COUNT(*)` counts all rows including NULLs
- `COUNT(col)` excludes NULL values
- `FILTER` clause for conditional aggregation
- `HAVING` filters after GROUP BY aggregation
