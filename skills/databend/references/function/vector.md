## Vector Functions

Vector similarity for AI/ML applications.

### Functions

| Function | Description |
|----------|-------------|
| `VECTOR_COSINE_DISTANCE(v1, v2)` | Cosine similarity |
| `VECTOR_L1_DISTANCE(v1, v2)` | Manhattan distance |
| `VECTOR_L2_DISTANCE(v1, v2)` | Euclidean distance |
| `VECTOR_INNER_PRODUCT(v1, v2)` | Dot product |
| `VECTOR_DIMS(v)` | Vector dimensions |
| `VECTOR_NORM(v)` | Vector magnitude |

### Examples

```sql
-- Create vector column
CREATE TABLE embeddings (
    id INT,
    vec ARRAY<FLOAT32>
);

-- Insert vector
INSERT INTO embeddings VALUES (1, [0.1, 0.2, 0.3, 0.4]);

-- Cosine similarity search
SELECT id, VECTOR_COSINE_DISTANCE(vec, [0.1, 0.2, 0.3, 0.4]) AS sim
FROM embeddings
ORDER BY sim ASC
LIMIT 10;

-- L2 distance
SELECT id, VECTOR_L2_DISTANCE(vec, [0.1, 0.2, 0.3, 0.4]) AS dist
FROM embeddings
ORDER BY dist ASC
LIMIT 10;

-- Vector dimensions
SELECT VECTOR_DIMS([1.0, 2.0, 3.0]);    → 3
SELECT VECTOR_NORM([3.0, 4.0]);          → 5.0
```

### Notes

- Vectors stored as `ARRAY<FLOAT32>` or `ARRAY<FLOAT64>`
- Use vector index for fast ANN search
- Cosine distance: 0 = identical, 2 = opposite
