## Numeric Functions

Mathematical operations on numbers.

### Functions

| Function | Description |
|----------|-------------|
| `ABS(x)` | Absolute value |
| `ROUND(x, d)`, `TRUNC(x, d)` | Round/truncate to d decimals |
| `CEIL(x)`, `FLOOR(x)` | Round up/down to integer |
| `POWER(x, y)`, `SQRT(x)` | Power and square root |
| `LOG(x)`, `LOG10(x)`, `LOG2(x)` | Logarithms |
| `EXP(x)` | e^x |
| `MOD(x, y)`, `x % y` | Modulo |
| `SIGN(x)` | -1, 0, or 1 |
| `RAND()`, `RAND(n)` | Random number |
| `PI()` | π constant |

### Examples

```sql
SELECT ABS(-5);                    → 5
SELECT ROUND(3.14159, 2);          → 3.14
SELECT CEIL(3.2), FLOOR(3.8);      → 4, 3
SELECT POWER(2, 10);               → 1024
SELECT SQRT(16);                   → 4
SELECT MOD(17, 5), 17 % 5;         → 2, 2
SELECT LOG(100), LOG10(100);       → 4.605, 2
SELECT RAND();                     → random 0-1
```
