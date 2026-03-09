## TRUNCATE TABLE

Remove all rows from a table instantly.

### Syntax

```sql
TRUNCATE [ TABLE ] [<database>.]<table>
```

### Examples

```sql
-- Truncate table
TRUNCATE TABLE users;

-- Truncate in specific database
TRUNCATE mydb.logs;
```

### Notes

- Faster than `DELETE` (no row-by-row processing)
- Cannot be rolled back (unlike DELETE)
- Resets auto-increment counters
- Preserves table structure and indexes
