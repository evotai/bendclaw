## TASK

Scheduled jobs for automated execution.

### Commands

```sql
CREATE TASK [ IF NOT EXISTS ] [<database>.]<name>
    [ WAREHOUSE = <warehouse> ]
    [ SCHEDULE = { <interval> | CRON '<cron>' } ]
    [ AFTER <task> [ , ... ] ]  -- run after other tasks
    [ WHEN <condition> ]        -- conditional execution
    [ ERROR_INTEGRATION = <name> ]
    [ COMMENT = '<comment>' ]
AS <sql>

ALTER TASK <name> { SUSPEND | RESUME | SET ... }
DROP TASK [ IF EXISTS ] <name>
SHOW TASKS
DESCRIBE TASK <name>
```

### Examples

```sql
-- Task running every hour
CREATE TASK hourly_cleanup
    WAREHOUSE = compute
    SCHEDULE = 1 HOUR
AS
    DELETE FROM logs WHERE created_at < DATEADD(hour, -24, NOW());

-- Task with CRON schedule
CREATE TASK daily_report
    SCHEDULE = CRON '0 8 * * *'
AS
    COPY INTO @reports FROM (SELECT * FROM generate_daily_report());

-- Task triggered by stream
CREATE TASK process_orders
    WAREHOUSE = compute
    AFTER orders_stream
    WHEN SYSTEM$STREAM_HAS_DATA('orders_stream')
AS
    INSERT INTO processed_orders SELECT * FROM orders_stream;

-- Manage task
ALTER TASK daily_report SUSPEND;
ALTER TASK daily_report RESUME;
DROP TASK daily_report;
```

### Notes

- Tasks run in specified warehouse
- `AFTER` creates task dependencies
- `WHEN` allows conditional execution
- Tasks are suspended by default after creation
