## UPDATE

Modify existing rows in a table.

### Syntax

```sql
UPDATE <table>
    SET <column> = <expr> [ , ... ]
    [ FROM <table> [ , ... ] ]
    [ WHERE <condition> ]
```

### Examples

```sql
-- Simple update
UPDATE users SET status = 'active' WHERE id = 1;

-- Update multiple columns
UPDATE users SET name = 'Alice', email = 'alice@new.com' WHERE id = 1;

-- Update with expression
UPDATE products SET price = price * 1.1 WHERE category = 'electronics';

-- Update with FROM clause
UPDATE orders o
SET status = 'shipped'
FROM customers c
WHERE o.customer_id = c.id AND c.vip = true;

-- Update all rows (no WHERE)
UPDATE users SET last_seen = NOW();
```

### Notes

- Without WHERE clause, updates all rows
- Supports FROM clause for joins
- Returns number of rows affected
