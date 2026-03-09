## String Functions

Functions for string manipulation.

### Common Functions

| Function | Description |
|----------|-------------|
| `CONCAT(s1, s2, ...)` | Concatenate strings |
| `CONCAT_WS(sep, s1, s2)` | Concatenate with separator |
| `SUBSTRING(s, start, len)` | Extract substring (1-indexed) |
| `LEFT(s, n)`, `RIGHT(s, n)` | First/last n characters |
| `LENGTH(s)`, `CHAR_LENGTH(s)` | Character count |
| `UPPER(s)`, `LOWER(s)` | Case conversion |
| `TRIM(s)`, `LTRIM(s)`, `RTRIM(s)` | Remove whitespace |
| `REPLACE(s, from, to)` | Replace all occurrences |
| `REVERSE(s)` | Reverse string |
| `POSITION(sub IN s)` | Find substring position |
| `SPLIT(s, delim)` | Split into array |
| `REGEXP_REPLACE(s, pattern, repl)` | Regex replace |
| `LIKE`, `RLIKE` | Pattern matching |

### Examples

```sql
-- Concatenation
SELECT CONCAT('Hello', ' ', 'World');           → 'Hello World'
SELECT CONCAT_WS(',', 'A', 'B', 'C');           → 'A,B,C'

-- Substring
SELECT SUBSTRING('Hello World', 1, 5);          → 'Hello'
SELECT LEFT('Hello', 3);                        → 'Hel'
SELECT RIGHT('Hello', 3);                       → 'llo'

-- Case and trim
SELECT UPPER('hello'), LOWER('HELLO');          → 'HELLO', 'hello'
SELECT TRIM('  hello  ');                       → 'hello'
SELECT LTRIM('  hello'), RTRIM('hello  ');      → 'hello', 'hello'

-- Replace
SELECT REPLACE('Hello', 'l', 'x');             → 'Hexxo'
SELECT REGEXP_REPLACE('abc123', '[0-9]', 'X');  → 'abcXXX'

-- Search and split
SELECT POSITION('World' IN 'Hello World');      → 7
SELECT SPLIT('A,B,C', ',');                     → ['A', 'B', 'C']

-- Pattern matching
SELECT 'hello' LIKE 'h%';                      → true
SELECT 'abc123' RLIKE '[0-9]+';                 → true
```

### Notes

- String indices are 1-based
- `CONCAT` returns NULL if any argument is NULL
- Use `||` operator for simple concatenation
