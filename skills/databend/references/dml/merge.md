## MERGE

Conditionally insert, update, or delete rows based on a join.

### Syntax

```sql
MERGE INTO <target_table>
USING <source> ON <condition>
WHEN MATCHED THEN
    UPDATE SET <column> = <expr> [ , ... ]
    | DELETE
WHEN NOT MATCHED THEN
    INSERT [ ( <column> [ , ... ] ) ] VALUES ( <expr> [ , ... ] )
```

### Examples

```sql
-- Merge with update and insert
MERGE INTO target t
USING source s ON t.id = s.id
WHEN MATCHED THEN UPDATE SET t.value = s.value
WHEN NOT MATCHED THEN INSERT (id, value) VALUES (s.id, s.value);

-- Merge with delete
MERGE INTO users u
USING inactive i ON u.id = i.user_id
WHEN MATCHED THEN DELETE;

-- Merge from stage
MERGE INTO users u
USING (SELECT $1, $2 FROM @stage) s ON u.id = s.$1
WHEN MATCHED THEN UPDATE SET u.name = s.$2;
```

### Notes

- Multiple WHEN clauses can be combined
- Supports subqueries in source
- Atomic operation
