## DateTime Functions

Functions for date and time manipulation.

### Common Functions

| Function | Description |
|----------|-------------|
| `NOW()`, `CURRENT_TIMESTAMP()` | Current timestamp |
| `CURRENT_DATE()`, `TODAY()` | Current date |
| `TO_DATE(s)`, `TO_TIMESTAMP(s)` | Convert string to date/timestamp |
| `DATE_ADD(unit, n, date)` | Add interval |
| `DATE_DIFF(unit, d1, d2)` | Difference between dates |
| `DATE_TRUNC(unit, date)` | Truncate to unit |
| `EXTRACT(part FROM date)` | Extract date part |
| `YEAR(d)`, `MONTH(d)`, `DAY(d)` | Extract parts |
| `DATE_FORMAT(d, fmt)` | Format date string |
| `TO_UNIX_TIMESTAMP(d)` | Convert to Unix timestamp |

### Date Parts

`YEAR`, `QUARTER`, `MONTH`, `WEEK`, `DAY`, `HOUR`, `MINUTE`, `SECOND`

### Examples

```sql
-- Current date/time
SELECT NOW(), CURRENT_DATE();                   → 2025-02-27 10:30:00, 2025-02-27
SELECT TODAY(), TOMORROW(), YESTERDAY();        → 2025-02-27, 2025-02-28, 2025-02-26

-- Conversion
SELECT TO_DATE('2025-01-15');                   → 2025-01-15
SELECT TO_TIMESTAMP('2025-01-15 10:30:00');     → 2025-01-15 10:30:00

-- Date arithmetic
SELECT DATE_ADD(DAY, 7, NOW());                 → 7 days from now
SELECT DATE_ADD(MONTH, 1, '2025-01-15');        → 2025-02-15
SELECT DATE_DIFF(DAY, '2025-01-01', NOW());     → days since Jan 1

-- Truncate
SELECT DATE_TRUNC(MONTH, NOW());               → 2025-02-01 00:00:00
SELECT DATE_TRUNC(WEEK, NOW());                → start of week

-- Extract parts
SELECT YEAR(NOW()), MONTH(NOW()), DAY(NOW());
SELECT EXTRACT(YEAR FROM NOW()), EXTRACT(MONTH FROM NOW());

-- Format
SELECT DATE_FORMAT(NOW(), '%Y-%m-%d');          → '2025-02-27'
SELECT DATE_FORMAT(NOW(), '%Y年%m月%d日');       → '2025年02月27日'

-- Unix timestamp
SELECT TO_UNIX_TIMESTAMP(NOW());                → 1740623400
```

### Notes

- `DATE_ADD`/`DATE_DIFF` units: `YEAR`, `QUARTER`, `MONTH`, `WEEK`, `DAY`, `HOUR`, `MINUTE`, `SECOND`
- `DATE_TRUNC` returns timestamp at start of period
