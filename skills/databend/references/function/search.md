## Search Functions

Full-text search and relevance scoring.

### Functions

| Function | Description |
|----------|-------------|
| `MATCH(columns, query)` | Full-text match |
| `SCORE()` | Relevance score |
| `QUERY(columns, query)` | Query with syntax |

### Examples

```sql
-- Basic full-text search
SELECT * FROM articles WHERE MATCH(content, 'databend');
SELECT * FROM articles WHERE MATCH(content, 'databend query');

-- With score
SELECT id, title, SCORE() AS relevance
FROM articles
WHERE MATCH(content, 'databend')
ORDER BY SCORE() DESC;

-- Multiple columns
SELECT * FROM docs WHERE MATCH(title || ' ' || body, 'search');

-- Query syntax
SELECT * FROM articles WHERE QUERY(content, 'databend AND (query OR sql)');
```

### Query Syntax

| Operator | Description |
|----------|-------------|
| `term` | Match term |
| `term1 AND term2` | Both terms |
| `term1 OR term2` | Either term |
| `"phrase"` | Exact phrase |
| `*` | Wildcard prefix |
| `NOT term` | Exclude term |

### Notes

- Requires fulltext index on column
- `SCORE()` only valid in SELECT with MATCH
