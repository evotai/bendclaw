## Hash Functions

Compute hash values for data.

### Functions

| Function | Description |
|----------|-------------|
| `MD5(s)` | 128-bit MD5 hash |
| `SHA1(s)` | 160-bit SHA-1 hash |
| `SHA2(s, bits)` | SHA-2 hash (224, 256, 384, 512 bits) |
| `SHA256(s)` | SHA-256 hash |
| `XXHASH32(s)`, `XXHASH64(s)` | XXHash (fast) |
| `SIPHASH(s)`, `SIPHASH64(s)` | SipHash |
| `BLAKE3(s)` | BLAKE3 hash |
| `CRC32(s)` | CRC32 checksum |

### Examples

```sql
SELECT MD5('hello');                    → '5d41402abc4b2a76b9719d911017c592'
SELECT SHA1('hello');                   → 'aaf4c61ddcc5e8a2dabede0f3b482cd9aea9434d'
SELECT SHA256('hello');                 → 64-char hex string
SELECT XXHASH64('hello');               → fast 64-bit hash
SELECT CRC32('hello');                  → 907060870
```

### Notes

- MD5/SHA1: legacy, avoid for security
- SHA256/BLAKE3: recommended for security
- XXHASH: fastest for non-crypto use
