## SEQUENCE

Auto-increment number generators.

### Commands

```sql
CREATE SEQUENCE [ IF NOT EXISTS ] <name>
    [ START = <start> ]
    [ INCREMENT = <step> ]

ALTER SEQUENCE <name> { SET START = <n> | SET INCREMENT = <n> }
DROP SEQUENCE <name>
SHOW SEQUENCES
```

### Usage

```sql
-- Next value
SELECT NEXTVAL(my_seq);

-- Use in INSERT
INSERT INTO users (id, name) VALUES (NEXTVAL(user_seq), 'Alice');

-- Use as default
CREATE TABLE orders (
    id BIGINT DEFAULT NEXTVAL(order_seq),
    user_id INT
);
```

### Examples

```sql
-- Create sequence
CREATE SEQUENCE user_id START = 1000 INCREMENT = 1;

-- Get next value
SELECT NEXTVAL(user_id);  → 1000
SELECT NEXTVAL(user_id);  → 1001

-- Create sequence with step
CREATE SEQUENCE even_numbers START = 0 INCREMENT = 2;
SELECT NEXTVAL(even_numbers);  → 0
SELECT NEXTVAL(even_numbers);  → 2

-- Drop sequence
DROP SEQUENCE user_id;
```

### Notes

- Sequences are independent of tables
- `AUTOINCREMENT` creates implicit sequence
- Dropping table with AUTOINCREMENT drops the sequence
