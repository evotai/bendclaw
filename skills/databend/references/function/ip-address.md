## IP Address Functions

Work with IPv4 addresses.

### Functions

| Function | Description |
|----------|-------------|
| `INET_ATON(ip)` | IPv4 string to integer |
| `INET_NTOA(int)` | Integer to IPv4 string |
| `TRY_INET_ATON(ip)` | Returns NULL on failure |
| `TRY_INET_NTOA(int)` | Returns NULL on failure |
| `IPV4_STRING_TO_NUM(ip)` | Same as INET_ATON |
| `IPV4_NUM_TO_STRING(int)` | Same as INET_NTOA |

### Examples

```sql
-- Convert IPv4 to integer
SELECT INET_ATON('192.168.1.1');         → 3232235777

-- Convert integer to IPv4
SELECT INET_NTOA(3232235777);            → '192.168.1.1'

-- Safe conversion
SELECT TRY_INET_ATON('invalid');        → NULL

-- Filter IP range
SELECT * FROM logs 
WHERE INET_ATON(ip) BETWEEN INET_ATON('192.168.0.0') AND INET_ATON('192.168.255.255');
```
