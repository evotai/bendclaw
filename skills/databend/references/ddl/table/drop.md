## DROP TABLE

Remove a table from the database.

### Syntax

```sql
DROP TABLE [ IF EXISTS ] [<database>.]<table>
```

### Examples

```sql
-- Drop table
DROP TABLE users;

-- Drop if exists (no error if missing)
DROP TABLE IF EXISTS temp_table;

-- Drop from specific database
DROP DATABASE mydb.public.users;
```

### Notes

- Can be recovered via `UNDROP TABLE` within retention period
- Use `VACUUM DROP TABLE` to permanently remove
- Foreign key constraints may block drop
