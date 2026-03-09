## Window Functions

Compute values over a window of rows related to the current row.

### Syntax

```sql
<function>() OVER (
    [ PARTITION BY <expr> [ , ... ] ]
    [ ORDER BY <expr> [ ASC | DESC ] [ , ... ] ]
    [ { ROWS | RANGE } BETWEEN <frame_start> AND <frame_end> ]
)
```

### Functions

| Function | Description |
|----------|-------------|
| `ROW_NUMBER()` | Sequential row number |
| `RANK()`, `DENSE_RANK()` | Rank with/without gaps |
| `NTILE(n)` | Divide into n buckets |
| `LAG(expr, offset)` | Previous row value |
| `LEAD(expr, offset)` | Next row value |
| `FIRST_VALUE(expr)`, `LAST_VALUE(expr)` | First/last in window |
| `NTH_VALUE(expr, n)` | Nth value in window |
| `CUME_DIST()`, `PERCENT_RANK()` | Distribution functions |

### Examples

```sql
-- Row number per partition
SELECT *, ROW_NUMBER() OVER (PARTITION BY dept ORDER BY salary DESC) AS rank
FROM employees;

-- Running total
SELECT *, SUM(amount) OVER (ORDER BY date) AS running_total
FROM sales;

-- Compare with previous row
SELECT *, LAG(price, 1) OVER (ORDER BY date) AS prev_price
FROM stocks;

-- Moving average
SELECT *, AVG(price) OVER (ORDER BY date ROWS BETWEEN 6 PRECEDING AND CURRENT ROW) AS ma7
FROM stocks;
```

### Frame Syntax

```sql
ROWS BETWEEN UNBOUNDED PRECEDING AND CURRENT ROW    -- from start to current
ROWS BETWEEN 6 PRECEDING AND 6 FOLLOWING            -- 13-row centered window
RANGE BETWEEN UNBOUNDED PRECEDING AND UNBOUNDED FOLLOWING  -- all rows
```
