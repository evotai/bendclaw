## CONNECTION

Secure storage for external service credentials.

### Commands

```sql
CREATE CONNECTION [ IF NOT EXISTS ] <name>
    STORAGE_TYPE = '<type>'
    [ <param> = '<value> [ , ... ] ]

DROP CONNECTION <name>
SHOW CONNECTIONS
DESCRIBE CONNECTION <name>
```

### Examples

```sql
-- S3 connection
CREATE CONNECTION s3_conn
    STORAGE_TYPE = 's3'
    AWS_ACCESS_KEY_ID = 'xxx'
    AWS_SECRET_ACCESS_KEY = 'xxx';

-- Use connection in stage
CREATE STAGE my_stage
    URL = 's3://bucket/path/'
    CONNECTION = (CONNECTION_NAME = 's3_conn');

-- GCS connection
CREATE CONNECTION gcs_conn
    STORAGE_TYPE = 'gcs'
    CREDENTIAL = '{"type":"service_account",...}';
```

### Notes

- Stores credentials securely
- Referenced by stages and external tables
- Connection credentials are masked in SHOW commands
