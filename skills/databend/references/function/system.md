## System Functions

Query system and table metadata.

### Functions

| Function | Description |
|----------|-------------|
| `FUSE_SNAPSHOT(table)` | List table snapshots |
| `FUSE_BLOCK(table)` | List table blocks |
| `FUSE_SEGMENT(table)` | List table segments |
| `FUSE_COLUMN(table)` | Column statistics |
| `FUSE_STATISTIC(table)` | Table statistics |
| `FUSE_TIME_TRAVEL_SIZE(table, time)` | Size at point in time |
| `CLUSTERING_INFORMATION(table)` | Clustering health |
| `FUSE_VIRTUAL_COLUMN(table)` | Virtual column info |

### Examples

```sql
-- List snapshots (for time travel)
SELECT * FROM FUSE_SNAPSHOT('mydb.users');

-- Block information
SELECT block_id, row_count, size FROM FUSE_BLOCK('mydb.users');

-- Clustering health
SELECT * FROM CLUSTERING_INFORMATION('mydb.users');

-- Table statistics
SELECT * FROM FUSE_STATISTIC('mydb.users');

-- Time travel size
SELECT FUSE_TIME_TRAVEL_SIZE('mydb.users', '2025-01-01');
```
