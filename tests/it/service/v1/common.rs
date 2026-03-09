use bendclaw::service::v1::common::ListQuery;
use bendclaw::service::v1::common::Paginated;

// ── ListQuery ──

#[test]
fn list_query_default_limit() {
    let q = ListQuery::default();
    assert_eq!(q.limit(), 50);
}

#[test]
fn list_query_limit_capped_at_200() {
    let q: ListQuery = serde_json::from_str(r#"{"limit": 999}"#).unwrap();
    assert_eq!(q.limit(), 200);
}

#[test]
fn list_query_custom_limit() {
    let q: ListQuery = serde_json::from_str(r#"{"limit": 10}"#).unwrap();
    assert_eq!(q.limit(), 10);
}

#[test]
fn list_query_offset_default_page_1() {
    let q = ListQuery::default();
    assert_eq!(q.offset(), 0);
}

#[test]
fn list_query_offset_page_2() {
    let q: ListQuery = serde_json::from_str(r#"{"page": 2}"#).unwrap();
    assert_eq!(q.offset(), 50); // (2-1) * 50
}

#[test]
fn list_query_offset_page_0_treated_as_1() {
    let q: ListQuery = serde_json::from_str(r#"{"page": 0}"#).unwrap();
    assert_eq!(q.offset(), 0);
}

#[test]
fn list_query_offset_with_custom_limit() {
    let q: ListQuery = serde_json::from_str(r#"{"limit": 20, "page": 3}"#).unwrap();
    assert_eq!(q.offset(), 40); // (3-1) * 20
}

#[test]
fn list_query_order_default_desc() {
    let q = ListQuery::default();
    assert_eq!(q.order(), "DESC");
}

#[test]
fn list_query_order_asc() {
    let q: ListQuery = serde_json::from_str(r#"{"sort_order": "asc"}"#).unwrap();
    assert_eq!(q.order(), "ASC");
}

#[test]
fn list_query_order_desc_explicit() {
    let q: ListQuery = serde_json::from_str(r#"{"sort_order": "desc"}"#).unwrap();
    assert_eq!(q.order(), "DESC");
}

#[test]
fn list_query_limit_as_string() {
    let q: ListQuery = serde_json::from_str(r#"{"limit": "25"}"#).unwrap();
    assert_eq!(q.limit(), 25);
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
fn paginated_total_pages_rounds_up() {
    let q: ListQuery = serde_json::from_str(r#"{"limit": 30}"#).unwrap();
    let p: Paginated<u32> = Paginated::new(vec![], &q, 100);
    assert_eq!(p.meta.total_pages, 4); // ceil(100/30)
}

#[test]
fn paginated_zero_total() {
    let q = ListQuery::default();
    let p: Paginated<u32> = Paginated::new(vec![], &q, 0);
    assert_eq!(p.meta.total_count, 0);
    assert_eq!(p.meta.total_pages, 0);
}
