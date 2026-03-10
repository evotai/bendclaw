use anyhow::Result;
use bendclaw::service::v1::common::ListQuery;
use bendclaw::service::v1::common::Paginated;

// ── ListQuery ──

#[test]
fn list_query_default_limit() {
    let q = ListQuery::default();
    assert_eq!(q.limit(), 50);
}

#[test]
fn list_query_limit_capped_at_200() -> Result<()> {
    let q: ListQuery = serde_json::from_str(r#"{"limit": 999}"#)?;
    assert_eq!(q.limit(), 200);
    Ok(())
}

#[test]
fn list_query_custom_limit() -> Result<()> {
    let q: ListQuery = serde_json::from_str(r#"{"limit": 10}"#)?;
    assert_eq!(q.limit(), 10);
    Ok(())
}

#[test]
fn list_query_offset_default_page_1() {
    let q = ListQuery::default();
    assert_eq!(q.offset(), 0);
}

#[test]
fn list_query_offset_page_2() -> Result<()> {
    let q: ListQuery = serde_json::from_str(r#"{"page": 2}"#)?;
    assert_eq!(q.offset(), 50); // (2-1) * 50
    Ok(())
}

#[test]
fn list_query_offset_page_0_treated_as_1() -> Result<()> {
    let q: ListQuery = serde_json::from_str(r#"{"page": 0}"#)?;
    assert_eq!(q.offset(), 0);
    Ok(())
}

#[test]
fn list_query_offset_with_custom_limit() -> Result<()> {
    let q: ListQuery = serde_json::from_str(r#"{"limit": 20, "page": 3}"#)?;
    assert_eq!(q.offset(), 40); // (3-1) * 20
    Ok(())
}

#[test]
fn list_query_order_default_desc() {
    let q = ListQuery::default();
    assert_eq!(q.order(), "DESC");
}

#[test]
fn list_query_order_asc() -> Result<()> {
    let q: ListQuery = serde_json::from_str(r#"{"sort_order": "asc"}"#)?;
    assert_eq!(q.order(), "ASC");
    Ok(())
}

#[test]
fn list_query_order_desc_explicit() -> Result<()> {
    let q: ListQuery = serde_json::from_str(r#"{"sort_order": "desc"}"#)?;
    assert_eq!(q.order(), "DESC");
    Ok(())
}

#[test]
fn list_query_limit_as_string() -> Result<()> {
    let q: ListQuery = serde_json::from_str(r#"{"limit": "25"}"#)?;
    assert_eq!(q.limit(), 25);
    Ok(())
}

// ── Paginated ──

#[test]
fn paginated_meta_defaults() {
    let q = ListQuery::default();
    let p: Paginated<String> = Paginated::new(vec!["a".into(), "b".into()], &q, 100);
    assert_eq!(p.meta.page, 1);
    assert_eq!(p.meta.limit, 50);
    assert_eq!(p.meta.total_count, 100);
    assert_eq!(p.meta.total_pages, 2);
    assert_eq!(p.data.len(), 2);
}

#[test]
fn paginated_total_pages_rounds_up() -> Result<()> {
    let q: ListQuery = serde_json::from_str(r#"{"limit": 30}"#)?;
    let p: Paginated<u32> = Paginated::new(vec![], &q, 100);
    assert_eq!(p.meta.total_pages, 4); // ceil(100/30)
    Ok(())
}

#[test]
fn paginated_zero_total() {
    let q = ListQuery::default();
    let p: Paginated<u32> = Paginated::new(vec![], &q, 0);
    assert_eq!(p.meta.total_count, 0);
    assert_eq!(p.meta.total_pages, 0);
}
