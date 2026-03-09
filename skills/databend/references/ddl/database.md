## DATABASE

Create and manage databases.

### Commands

```sql
CREATE DATABASE [ IF NOT EXISTS ] <name>
DROP DATABASE [ IF EXISTS ] <name>
SHOW DATABASES
SHOW CREATE DATABASE <name>
```

### Examples

```sql
-- Create database
CREATE DATABASE mydb;

-- Create if not exists
CREATE DATABASE IF NOT EXISTS analytics;

-- Drop database
DROP DATABASE mydb;

-- List databases
SHOW DATABASES;

-- Show create statement
SHOW CREATE DATABASE mydb;
```

### Notes

- Creates default `public` schema
- Dropping database removes all tables
- Database names must be unique
