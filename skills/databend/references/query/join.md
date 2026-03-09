## JOIN

Combine rows from multiple tables.

### Syntax

```sql
<left_table> [ <join_type> ] JOIN <right_table>
    ON <condition>
    | USING (<column> [ , ... ])

<join_type> := { [ INNER ] | LEFT [ OUTER ] | RIGHT [ OUTER ] | FULL [ OUTER ] | CROSS }
```

### Join Types

| Type | Description |
|------|-------------|
| `INNER JOIN` | Matching rows only |
| `LEFT JOIN` | All left rows, matching right |
| `RIGHT JOIN` | All right rows, matching left |
| `FULL JOIN` | All rows from both |
| `CROSS JOIN` | Cartesian product |

### Examples

```sql
-- Inner join
SELECT u.name, o.total
FROM users u
INNER JOIN orders o ON u.id = o.user_id;

-- Left join (include users without orders)
SELECT u.name, o.total
FROM users u
LEFT JOIN orders o ON u.id = o.user_id;

-- Multiple joins
SELECT u.name, o.total, p.name AS product
FROM users u
JOIN orders o ON u.id = o.user_id
JOIN products p ON o.product_id = p.id;

-- Cross join
SELECT a.name, b.category
FROM products a
CROSS JOIN categories b;

-- Join with USING
SELECT u.name, o.total
FROM users u
JOIN orders o USING (user_id);
```

### Notes

- Use explicit `ON` clause for clarity
- `USING` requires same column name in both tables
