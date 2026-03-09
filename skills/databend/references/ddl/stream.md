## STREAM

Change data capture (CDC) for tracking table changes.

### Commands

```sql
CREATE STREAM [ IF NOT EXISTS ] [<database>.]<name>
    ON TABLE [<database>.]<table>
    [ AT ( { HEAD | <timestamp> | <snapshot_id> } ) ]
    [ APPEND_ONLY = true | false ]

DROP STREAM [ IF EXISTS ] [<database>.]<name>
SHOW STREAMS
DESCRIBE STREAM <name>
```

### Examples

```sql
-- Create stream on table
CREATE STREAM orders_stream ON TABLE orders;

-- Append-only stream (no DELETE/UPDATE tracking)
CREATE STREAM orders_append ON TABLE orders APPEND_ONLY = true;

-- Stream from specific point
CREATE STREAM orders_from_today ON TABLE orders
    AT (TIMESTAMP => CURRENT_TIMESTAMP());

-- Consume stream (shows changes since last check)
SELECT * FROM orders_stream;

-- Consume and mark as read
SELECT * FROM orders_stream CONSUME;
```

### Stream Columns

| Column | Description |
|--------|-------------|
| `$change_type` | INSERT, DELETE, UPDATE |
| `$is_update` | True if UPDATE |
| Original columns | Table data |

### Notes

- `CONSUME` marks stream as consumed
- Streams expire after configured retention
- APPEND_ONLY is more efficient for INSERT-only tables
