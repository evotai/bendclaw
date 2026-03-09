## CREATE TABLE

Create a new table.

### Syntax

```sql
CREATE [ OR REPLACE ] TABLE [ IF NOT EXISTS ] [<database>.]<table>
(
    <column> <type> [ NULL | NOT NULL ]
                   [ DEFAULT <expr> | { AUTOINCREMENT | IDENTITY } [ ... ] ]
                   [ AS (<expr>) STORED | VIRTUAL ]
                   [ COMMENT '<comment>' ]
    [ , ... ]
)
[ CLUSTER BY (<column> [ , ... ]) ]
[ TRANSIENT ]
```

### Column Types

| Type | Description |
|------|-------------|
| `INT`, `BIGINT`, `FLOAT`, `DOUBLE` | Numeric |
| `VARCHAR(n)`, `TEXT` | String |
| `BOOLEAN` | Boolean |
| `DATE`, `TIMESTAMP`, `TIMESTAMPTZ` | DateTime |
| `DECIMAL(p,s)` | Exact numeric |
| `ARRAY`, `MAP`, `VARIANT` | Semi-structured |

### Examples

```sql
-- Basic table
CREATE TABLE users (
    id INT,
    name VARCHAR(100),
    email VARCHAR(200)
);

-- With defaults and constraints
CREATE TABLE products (
    id BIGINT AUTOINCREMENT,
    name VARCHAR(100) NOT NULL,
    price DECIMAL(10,2) DEFAULT 0.00,
    created_at TIMESTAMP DEFAULT NOW()
);

-- With cluster key
CREATE TABLE events (
    user_id INT,
    event_time TIMESTAMP,
    event_type VARCHAR(50)
) CLUSTER BY (user_id, event_time);

-- Transient table (no Time Travel)
CREATE TRANSIENT TABLE temp_data (id INT);

-- Create from query
CREATE TABLE archive AS SELECT * FROM users WHERE active = false;

-- Create like existing table
CREATE TABLE users_copy LIKE users;

-- Computed column
CREATE TABLE orders (
    id INT,
    price FLOAT,
    quantity INT,
    total FLOAT AS (price * quantity) STORED
);
```

### Notes

- All columns are nullable by default
- `CLUSTER BY` improves query performance
- `TRANSIENT` tables don't support Time Travel
- Computed columns can be `STORED` or `VIRTUAL`
