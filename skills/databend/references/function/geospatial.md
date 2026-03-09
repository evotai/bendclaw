## Geospatial Functions

Work with geographic and spatial data.

### Geometry Creation

| Function | Description |
|----------|-------------|
| `ST_POINT(x, y)` | Create point |
| `ST_MAKEPOINT(x, y)` | Same as ST_POINT |
| `ST_LINEFROMTEXT(wkt)` | Create line from WKT |
| `ST_POLYGON(wkt)` | Create polygon |
| `ST_GEOMFROMTEXT(wkt)` | Create geometry from WKT |

### Geometry Operations

| Function | Description |
|----------|-------------|
| `ST_X(geom)`, `ST_Y(geom)` | Extract coordinates |
| `ST_DISTANCE(geom1, geom2)` | Distance between geometries |
| `ST_CONTAINS(geom1, geom2)` | Containment test |
| `ST_LENGTH(geom)` | Line length |
| `ST_AREA(geom)` | Polygon area |
| `ST_ASTEXT(geom)` | Convert to WKT |
| `ST_ASGEOJSON(geom)` | Convert to GeoJSON |
| `ST_SRID(geom, srid)` | Set SRID |

### H3 Functions

| Function | Description |
|----------|-------------|
| `GEO_TO_H3(lng, lat, res)` | Geo to H3 cell |
| `H3_TO_GEO(h3)` | H3 to lat/lng |
| `H3_TO_STRING(h3)` | H3 to string |
| `H3_DISTANCE(h1, h2)` | Grid distance |
| `H3_K_RING(h3, k)` | K-ring neighbors |

### Examples

```sql
-- Create point
SELECT ST_POINT(-73.985, 40.748);      → POINT(-73.985 40.748)

-- Distance
SELECT ST_DISTANCE(ST_POINT(0, 0), ST_POINT(1, 1));

-- Containment
SELECT ST_CONTAINS(ST_POLYGON('...'), ST_POINT(1, 1));

-- H3 encoding
SELECT GEO_TO_H3(-73.985, 40.748, 9);  → H3 cell ID
SELECT H3_TO_STRING(GEO_TO_H3(-73.985, 40.748, 9));
```
