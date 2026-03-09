## UUID Functions

Generate and work with UUIDs.

### Functions

| Function | Description |
|----------|-------------|
| `UUID()` | Generate random UUID v4 |
| `GEN_RANDOM_UUID()` | Same as UUID() |
| `UUID_STRING()` | UUID as string |

### Examples

```sql
-- Generate UUID
SELECT UUID();                          → '550e8400-e29b-41d4-a716-446655440000'
SELECT GEN_RANDOM_UUID();               → same

-- Use in table
CREATE TABLE users (
    id VARCHAR(36) DEFAULT UUID(),
    name VARCHAR
);

-- Insert with auto-generated UUID
INSERT INTO users (name) VALUES ('Alice');
```

### Notes

- UUID v4 (random) is the default
- Returns 36-char string: 8-4-4-4-12 format
