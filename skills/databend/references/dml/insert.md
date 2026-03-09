## INSERT

Insert rows into a table.

### Syntax

```sql
INSERT { OVERWRITE [ INTO ] | INTO } <table>
    [ ( <column> [ , ... ] ) ]
    { VALUES ( <value> | DEFAULT ) [ , ... ] | <query> }
```

| Parameter | Description |
|-----------|-------------|
| `OVERWRITE` | Truncate table before insert |
| `VALUES` | Literal values or DEFAULT |
| `<query>` | SELECT statement |

### Examples

```sql
-- Insert single row
INSERT INTO users (id, name, email) VALUES (1, 'Alice', 'alice@example.com');

-- Insert multiple rows
INSERT INTO users VALUES 
    (1, 'Alice', 'alice@example.com'),
    (2, 'Bob', 'bob@example.com');

-- Insert with DEFAULT values
INSERT INTO users (id, name) VALUES (3, DEFAULT);

-- Insert from query
INSERT INTO archive SELECT * FROM users WHERE active = false;

-- Overwrite (replace all data)
INSERT OVERWRITE INTO users VALUES (1, 'New User');

-- Insert from stage
INSERT INTO users (id, name)
SELECT $1, $2 FROM @my_stage (FILE_FORMAT => 'parquet');
```

### Notes

- Atomic operation: either all rows succeed or none
- Column order matters when columns not specified
- Type casting happens automatically when needed
