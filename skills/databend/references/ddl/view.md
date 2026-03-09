## VIEW

Virtual tables based on queries.

### Commands

```sql
CREATE [ OR REPLACE ] VIEW [<database>.]<name> AS <query>
DROP VIEW [ IF EXISTS ] [<database>.]<name>
SHOW VIEWS [ FROM <database> ]
SHOW CREATE VIEW <name>
```

### Examples

```sql
-- Create view
CREATE VIEW active_users AS
    SELECT * FROM users WHERE active = true;

-- Replace existing view
CREATE OR REPLACE VIEW active_users AS
    SELECT id, name, email FROM users WHERE active = true;

-- View in specific database
CREATE VIEW analytics.daily_stats AS
    SELECT DATE(created_at) AS date, COUNT(*) AS count
    FROM events
    GROUP BY date;

-- Drop view
DROP VIEW active_users;

-- List views
SHOW VIEWS;
SHOW VIEWS FROM mydb;
```

### Notes

- Views are read-only (cannot INSERT/UPDATE/DELETE)
- View query runs on each access
- Use for frequently-used queries
