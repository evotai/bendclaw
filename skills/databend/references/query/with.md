## WITH (CTE)

Common Table Expressions for complex queries.

### Syntax

```sql
WITH <cte_name> [ ( <column> [ , ... ] ) ] AS (
    <query>
)
[ , <cte_name2> AS (...) ]
<main_query>
```

### Examples

```sql
-- Simple CTE
WITH active_users AS (
    SELECT * FROM users WHERE active = true
)
SELECT * FROM active_users;

-- Multiple CTEs
WITH 
monthly_sales AS (
    SELECT DATE_TRUNC('month', created_at) AS month, SUM(total) AS revenue
    FROM orders
    GROUP BY month
),
avg_sales AS (
    SELECT AVG(revenue) AS avg FROM monthly_sales
)
SELECT * FROM monthly_sales WHERE revenue > (SELECT avg FROM avg_sales);

-- CTE with column aliases
WITH user_stats (id, order_count, total_spent) AS (
    SELECT user_id, COUNT(*), SUM(total)
    FROM orders
    GROUP BY user_id
)
SELECT * FROM user_stats WHERE order_count > 10;

-- Recursive CTE
WITH RECURSIVE hierarchy AS (
    SELECT id, parent_id, 1 AS level
    FROM categories WHERE parent_id IS NULL
    UNION ALL
    SELECT c.id, c.parent_id, h.level + 1
    FROM categories c
    JOIN hierarchy h ON c.parent_id = h.id
)
SELECT * FROM hierarchy;

-- CTE in DML
WITH new_data AS (
    SELECT * FROM @stage WHERE $1 > 100
)
INSERT INTO target SELECT * FROM new_data;
```

### Notes

- CTEs improve readability for complex queries
- Can reference previous CTEs in same WITH clause
- Support recursive queries with `RECURSIVE` keyword
