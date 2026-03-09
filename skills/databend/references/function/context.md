## Context Functions

Return session and connection context information.

### Functions

| Function | Description |
|----------|-------------|
| `CURRENT_USER()` | Current username |
| `CURRENT_DATABASE()`, `DATABASE()` | Current database |
| `CURRENT_CATALOG()` | Current catalog |
| `CURRENT_TIMESTAMP()`, `NOW()` | Current timestamp |
| `CURRENT_DATE()` | Current date |
| `CONNECTION_ID()` | Connection identifier |
| `LAST_QUERY_ID()` | Last query ID |
| `VERSION()` | Databend version |

### Examples

```sql
SELECT CURRENT_USER();                 → 'admin'
SELECT CURRENT_DATABASE();              → 'default'
SELECT VERSION();                        → 'Databend Query v1.2.123'
SELECT CONNECTION_ID();                 → 12345

-- Use in queries
SELECT * FROM audit_log WHERE user = CURRENT_USER();
SELECT DATABASE() AS current_db;
```
