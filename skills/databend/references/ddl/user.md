## USER

User accounts and role-based access control.

### User Commands

```sql
CREATE USER <name> IDENTIFIED [ BY 'password' | WITH <auth_type> ]
ALTER USER <name> [ SET password = 'new' | ... ]
DROP USER <name>
SHOW USERS
```

### Role Commands

```sql
CREATE ROLE <name>
DROP ROLE <name>
SHOW ROLES
GRANT ROLE <role> TO USER <user>
REVOKE ROLE <role> FROM USER <user>
```

### Grant/Revoke Privileges

```sql
GRANT { ALL | <privilege> [ , ... ] } ON { DATABASE <db> | TABLE <db>.<table> | ALL * } TO <role>
REVOKE { ALL | <privilege> [ , ... ] } ON { DATABASE <db> | TABLE <table> } FROM <role>
```

### Privileges

| Privilege | Description |
|-----------|-------------|
| `SELECT` | Read data |
| `INSERT`, `UPDATE`, `DELETE` | Modify data |
| `CREATE`, `DROP`, `ALTER` | DDL operations |
| `ALL` | All privileges |

### Examples

```sql
-- Create user
CREATE USER analyst IDENTIFIED BY 'secret123';

-- Create role
CREATE ROLE analytics_read;

-- Grant privileges
GRANT SELECT ON DATABASE analytics TO ROLE analytics_read;
GRANT SELECT, INSERT ON DATABASE staging TO ROLE analytics_read;

-- Assign role to user
GRANT ROLE analytics_read TO USER analyst;

-- Show grants
SHOW GRANTS FOR USER analyst;
SHOW GRANTS FOR ROLE analytics_read;

-- Revoke
REVOKE SELECT ON DATABASE analytics FROM ROLE analytics_read;
```

### Notes

- Users can have multiple roles
- Default role set with `ALTER USER ... SET DEFAULT ROLE`
- Use roles for permission management, not individual users
