## INDEX

Table indexes for query performance.

### Cluster Key

Primary sort order for table data.

```sql
-- Create with cluster key
CREATE TABLE events (
    user_id INT,
    event_time TIMESTAMP,
    event_type VARCHAR
) CLUSTER BY (user_id, event_time);

-- Add cluster key to existing table
ALTER TABLE events CLUSTER BY (user_id, event_time);

-- Remove cluster key
ALTER TABLE events DROP CLUSTER KEY;
```

### Inverted Index

For semi-structured data queries.

```sql
CREATE INVERTED INDEX <name> ON <table>(<column>)
DROP INVERTED INDEX <name> ON <table>
```

### Fulltext Index

For full-text search.

```sql
CREATE FULLTEXT INDEX <name> ON <table>(<column>)
DROP FULLTEXT INDEX <name> ON <table>
```

### Vector Index

For approximate nearest neighbor search.

```sql
CREATE VECTOR INDEX <name> ON <table>(<vector_column>)
DROP VECTOR INDEX <name> ON <table>
```

### Examples

```sql
-- Fulltext index for search
CREATE FULLTEXT INDEX idx_content ON articles(content);
SELECT * FROM articles WHERE MATCH(content, 'search term');

-- Vector index for similarity
CREATE VECTOR INDEX idx_emb ON embeddings(vec);
SELECT * FROM embeddings ORDER BY VECTOR_L2_DISTANCE(vec, [1,2,3]) LIMIT 10;

-- Inverted index for JSON
CREATE INVERTED INDEX idx_data ON logs(data);
SELECT * FROM logs WHERE data:key = 'value';
```

### Notes

- Cluster key: improves range queries and joins
- Fulltext: required for MATCH function
- Vector: enables fast ANN search
