//! Integration tests for DatabendTable — generic CRUD layer.

use anyhow::Context as _;
use anyhow::Result;
use bendclaw::storage::sql::SqlVal;
use bendclaw::storage::table::DatabendTable;
use bendclaw::storage::table::RowMapper;
use bendclaw::storage::table::Where;

#[derive(Debug, Clone)]
struct Row {
    id: String,
    name: String,
}

struct RowMap;

impl RowMapper for RowMap {
    type Entity = Row;
    fn columns(&self) -> &str {
        "id, name"
    }
    fn parse(&self, row: &serde_json::Value) -> bendclaw::base::Result<Row> {
        let arr = row
            .as_array()
            .ok_or_else(|| bendclaw::base::ErrorCode::internal("expected array row"))?;
        Ok(Row {
            id: arr
                .first()
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string(),
            name: arr
                .get(1)
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string(),
        })
    }
}

async fn setup(table: &str) -> Result<(DatabendTable<RowMap>, crate::common::setup::TestContext)> {
    let ctx = crate::common::setup::TestContext::setup().await?;
    let pool = ctx.pool()?;
    pool.exec(&format!(
        "CREATE TABLE `{table}` (id VARCHAR, name VARCHAR)"
    ))
    .await?;
    let tbl = DatabendTable::new(pool, table, RowMap);
    Ok((tbl, ctx))
}

#[tokio::test]
async fn table_crud_roundtrip() -> Result<()> {
    let (tbl, _ctx) = setup("t_crud").await?;

    // insert + get
    tbl.insert(&[("id", SqlVal::Str("r1")), ("name", SqlVal::Str("alice"))])
        .await?;
    let row = tbl
        .get(&[Where("id", SqlVal::Str("r1"))])
        .await?
        .context("expected row")?;
    assert_eq!(row.id, "r1");
    assert_eq!(row.name, "alice");

    // list
    tbl.insert(&[("id", SqlVal::Str("r2")), ("name", SqlVal::Str("bob"))])
        .await?;
    let rows = tbl.list(&[], "id", 100).await?;
    assert_eq!(rows.len(), 2);

    // delete
    tbl.delete(&[Where("id", SqlVal::Str("r1"))]).await?;
    assert!(tbl.get(&[Where("id", SqlVal::Str("r1"))]).await?.is_none());

    // aggregate
    let agg = tbl.aggregate("COUNT(*) AS cnt", None).await?;
    let cnt: u64 = agg
        .context("expected agg row")?
        .as_array()
        .and_then(|a| a.first())
        .and_then(|v| v.as_str())
        .and_then(|s| s.parse().ok())
        .unwrap_or(0);
    assert_eq!(cnt, 1);

    Ok(())
}

#[tokio::test]
async fn table_upsert_and_batch() -> Result<()> {
    let (tbl, _ctx) = setup("t_upsert_batch").await?;

    // upsert insert
    tbl.upsert(
        &[("id", SqlVal::Str("u1")), ("name", SqlVal::Str("orig"))],
        "id",
    )
    .await?;
    let row = tbl
        .get(&[Where("id", SqlVal::Str("u1"))])
        .await?
        .context("expected row")?;
    assert_eq!(row.name, "orig");

    // upsert update
    tbl.upsert(
        &[("id", SqlVal::Str("u1")), ("name", SqlVal::Str("updated"))],
        "id",
    )
    .await?;
    let row2 = tbl
        .get(&[Where("id", SqlVal::Str("u1"))])
        .await?
        .context("expected row")?;
    assert_eq!(row2.name, "updated");

    // batch insert
    tbl.insert_batch(&["id", "name"], &[
        vec![SqlVal::Str("b1"), SqlVal::Str("batch")],
        vec![SqlVal::Str("b2"), SqlVal::Str("batch")],
    ])
    .await?;
    let rows = tbl
        .list(&[Where("name", SqlVal::Str("batch"))], "id", 100)
        .await?;
    assert_eq!(rows.len(), 2);

    Ok(())
}
