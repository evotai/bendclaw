## SELECT

Query data from tables.

### Syntax

```sql
SELECT [ ALL | DISTINCT ] <expr> [ [ AS ] <alias> ] [ , ... ]
    [ FROM <table> [ [ AS ] <alias> ] ]
    [ WHERE <condition> ]
    [ GROUP BY <expr> [ , ... ] [ HAVING <condition> ] ]
    [ ORDER BY <expr> [ ASC | DESC ] [ , ... ] ]
    [ LIMIT <n> [ OFFSET <offset> ] ]
    [ AT { <timestamp> | HEAD } ]
```

### Examples

```sql
-- Basic query
SELECT id, name, email FROM users;

-- Filter with WHERE
SELECT * FROM orders WHERE total > 100 AND status = 'completed';

-- Distinct values
SELECT DISTINCT category FROM products;

-- Aggregate with GROUP BY
SELECT category, COUNT(*) AS count, AVG(price) AS avg_price
FROM products
GROUP BY category
HAVING COUNT(*) > 5;

-- Order and limit
SELECT * FROM users ORDER BY created_at DESC LIMIT 10;

-- Pagination
SELECT * FROM users ORDER BY id LIMIT 20 OFFSET 40;

-- Time travel query
SELECT * FROM users AT (TIMESTAMP => '2025-01-01 00:00:00');

-- Query at specific snapshot
SELECT * FROM users AT (HEAD);
```

### Notes

- `AT` clause enables Time Travel queries
- `HAVING` filters after aggregation
- `OFFSET` is zero-based
