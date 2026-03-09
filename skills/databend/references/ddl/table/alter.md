## ALTER TABLE

Modify table structure.

### Syntax

```sql
ALTER TABLE [<database>.]<table>
    { ADD COLUMN <column> <type> [ FIRST | AFTER <col> ]
    | DROP COLUMN <column>
    | RENAME COLUMN <old> TO <new>
    | MODIFY COLUMN <column> [ SET DEFAULT <expr> | DROP DEFAULT ]
    | RENAME TO <new_name>
    | CLUSTER BY (<column> [ , ... ])
    | DROP CLUSTER KEY
    }
```

### Examples

```sql
-- Add column
ALTER TABLE users ADD COLUMN phone VARCHAR(20);

-- Add column at specific position
ALTER TABLE users ADD COLUMN middle_name VARCHAR(50) AFTER first_name;

-- Drop column
ALTER TABLE users DROP COLUMN middle_name;

-- Rename column
ALTER TABLE users RENAME COLUMN phone TO phone_number;

-- Set default value
ALTER TABLE users MODIFY COLUMN status SET DEFAULT 'active';

-- Rename table
ALTER TABLE users RENAME TO customers;

-- Add cluster key
ALTER TABLE events CLUSTER BY (user_id, event_time);

-- Remove cluster key
ALTER TABLE events DROP CLUSTER KEY;
```

### Notes

- Most operations are instant metadata changes
- Dropping columns doesn't immediately reclaim space
