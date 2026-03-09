## DELETE

Remove rows from a table.

### Syntax

```sql
DELETE FROM <table> [ WHERE <condition> ]
```

### Examples

```sql
-- Delete with condition
DELETE FROM users WHERE id = 1;

-- Delete multiple rows
DELETE FROM logs WHERE created_at < '2025-01-01';

-- Delete with subquery
DELETE FROM orders WHERE customer_id IN (SELECT id FROM inactive_customers);

-- Delete all rows (truncate alternative)
DELETE FROM temp_table;
```

### Notes

- Without WHERE clause, deletes all rows
- Atomic operation
- Returns number of rows deleted
