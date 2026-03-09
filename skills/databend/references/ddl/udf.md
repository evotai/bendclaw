## UDF (User-Defined Function)

Custom functions in SQL or external languages.

### SQL UDF

```sql
CREATE [ OR REPLACE ] FUNCTION <name>([ <param> <type> [ , ... ] ])
    RETURNS <type>
    AS <expression>

DROP FUNCTION <name>
SHOW USER FUNCTIONS
```

### Examples

```sql
-- Simple SQL UDF
CREATE FUNCTION add_tax(price FLOAT)
    RETURNS FLOAT
    AS price * 1.1;

-- Use UDF
SELECT add_tax(100);  → 110

-- String manipulation
CREATE FUNCTION format_name(first VARCHAR, last VARCHAR)
    RETURNS VARCHAR
    AS CONCAT(first, ' ', UPPER(last));

-- Drop function
DROP FUNCTION add_tax;
```

### External UDF

For Python/Rust/Go functions deployed separately.

```sql
CREATE FUNCTION <name>(...)
    RETURNS <type>
    LANGUAGE <language>
    HANDLER = '<handler>'
    ADDRESS = '<endpoint>'
```

### Notes

- SQL UDFs are inline expressions
- External UDFs call external service
- Functions are schema-level objects
