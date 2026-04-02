use anyhow::Result;
use bendclaw::storage::sql::SqlVal;
use bendclaw::storage::table::DatabendTable;
use bendclaw::storage::table::RowMapper;
use bendclaw::storage::table::Where;

use crate::common::fake_databend::paged_rows;
use crate::common::fake_databend::FakeDatabend;
use crate::common::fake_databend::FakeDatabendCall;

#[derive(Clone)]
struct TextMapper;

impl RowMapper for TextMapper {
    type Entity = String;

    fn columns(&self) -> &str {
        "value"
    }

    fn parse(&self, row: &serde_json::Value) -> bendclaw::types::Result<Self::Entity> {
        Ok(row[0].as_str().unwrap_or_default().to_string())
    }
}

#[tokio::test]
async fn table_get_builds_scoped_select_sql() -> Result<()> {
    let fake = FakeDatabend::new(|sql, database| {
        assert_eq!(database, None);
        assert_eq!(sql, "SELECT value FROM demo WHERE id = 'row-1' LIMIT 1");
        Ok(paged_rows(&[&["value-1"]], None, None))
    });
    let table = DatabendTable::new(fake.pool(), "demo", TextMapper);

    let value = table
        .get(&[Where("id", SqlVal::Str("row-1"))])
        .await?
        .expect("row should exist");

    assert_eq!(value, "value-1");
    assert_eq!(fake.calls(), vec![FakeDatabendCall::Query {
        sql: "SELECT value FROM demo WHERE id = 'row-1' LIMIT 1".to_string(),
        database: None,
    }]);
    Ok(())
}

#[tokio::test]
async fn table_insert_batch_skips_empty_rows() -> Result<()> {
    let fake = FakeDatabend::new(|_sql, _database| {
        panic!("empty batch should not issue any query");
    });
    let table = DatabendTable::new(fake.pool(), "demo", TextMapper);

    table.insert_batch(&["value"], &[]).await?;

    assert!(fake.calls().is_empty());
    Ok(())
}

#[tokio::test]
async fn table_get_uses_cache_when_enabled() -> Result<()> {
    let fake = FakeDatabend::new(|sql, _database| {
        assert_eq!(sql, "SELECT value FROM demo WHERE id = 'row-1' LIMIT 1");
        Ok(paged_rows(&[&["value-1"]], None, None))
    });
    let table = DatabendTable::new(fake.pool(), "demo", TextMapper)
        .with_ttl_cache(std::time::Duration::from_secs(60));

    let first = table.get(&[Where("id", SqlVal::Str("row-1"))]).await?;
    let second = table.get(&[Where("id", SqlVal::Str("row-1"))]).await?;

    assert_eq!(first.as_deref(), Some("value-1"));
    assert_eq!(second.as_deref(), Some("value-1"));
    assert_eq!(fake.calls().len(), 1);
    Ok(())
}

#[tokio::test]
async fn table_exec_raw_invalidates_cache() -> Result<()> {
    let query_count = std::sync::Arc::new(std::sync::Mutex::new(0usize));
    let query_count_cloned = query_count.clone();
    let fake = FakeDatabend::new(move |sql, _database| {
        if sql.starts_with("SELECT") {
            *query_count_cloned.lock().expect("query count lock") += 1;
            Ok(paged_rows(&[&["value-1"]], None, None))
        } else {
            Ok(paged_rows(&[], None, None))
        }
    });
    let table = DatabendTable::new(fake.pool(), "demo", TextMapper)
        .with_ttl_cache(std::time::Duration::from_secs(60));

    let _ = table.get(&[Where("id", SqlVal::Str("row-1"))]).await?;
    table
        .exec_raw("UPDATE demo SET value = 'x' WHERE id = 'row-1'")
        .await?;
    let _ = table.get(&[Where("id", SqlVal::Str("row-1"))]).await?;

    assert_eq!(*query_count.lock().expect("query count lock"), 2);
    Ok(())
}

#[tokio::test]
async fn table_does_not_cache_search_queries() -> Result<()> {
    let fake = FakeDatabend::new(|_sql, _database| Ok(paged_rows(&[&["value-1"]], None, None)));
    let table = DatabendTable::new(fake.pool(), "demo", TextMapper)
        .with_ttl_cache(std::time::Duration::from_secs(60));

    let _ = table
        .list_where("QUERY('value:abc')", "SCORE() DESC", 10)
        .await?;
    let _ = table
        .list_where("QUERY('value:abc')", "SCORE() DESC", 10)
        .await?;

    assert_eq!(fake.calls().len(), 2);
    Ok(())
}
