use anyhow::Result;
use bendclaw::storage::trace::TraceListFilter;
use bendclaw::storage::SpanRepo;
use bendclaw::storage::TraceRepo;

use crate::common::fake_databend::paged_rows;
use crate::common::fake_databend::FakeDatabend;

fn trace_row() -> Vec<serde_json::Value> {
    vec![
        "trace-1",
        "run-1",
        "sess-1",
        "agent-1",
        "user-1",
        "chat",
        "completed",
        "150",
        "100",
        "50",
        "0.005",
        "2026-03-11T00:00:00Z",
        "2026-03-11T00:01:00Z",
    ]
    .into_iter()
    .map(|s| serde_json::Value::String(s.to_string()))
    .collect()
}

fn span_row() -> Vec<serde_json::Value> {
    vec![
        "span-1",
        "trace-1",
        "",
        "gpt-4o",
        "llm",
        "main",
        "completed",
        "120",
        "80",
        "100",
        "50",
        "0",
        "0.003",
        "",
        "",
        "summary text",
        "{}",
        "2026-03-11T00:00:00Z",
    ]
    .into_iter()
    .map(|s| serde_json::Value::String(s.to_string()))
    .collect()
}

fn agg_row(vals: &[&str]) -> bendclaw::storage::pool::QueryResponse {
    paged_rows(&[&vals.iter().map(|s| *s).collect::<Vec<_>>()], None, None)
}

#[tokio::test]
async fn trace_repo_insert_generates_valid_sql() -> Result<()> {
    let fake = FakeDatabend::new(|sql, _db| {
        assert!(sql.starts_with("INSERT INTO traces"));
        assert!(sql.contains("trace_id"));
        assert!(sql.contains("NOW()"));
        Ok(paged_rows(&[], None, None))
    });
    let repo = TraceRepo::new(fake.pool());
    let record = bendclaw::storage::TraceRecord {
        trace_id: "t-1".into(),
        run_id: "r-1".into(),
        session_id: "s-1".into(),
        agent_id: "a-1".into(),
        user_id: "u-1".into(),
        name: "chat".into(),
        status: "running".into(),
        duration_ms: 0,
        input_tokens: 0,
        output_tokens: 0,
        total_cost: 0.0,
        created_at: String::new(),
        updated_at: String::new(),
    };
    repo.insert(&record).await?;
    assert_eq!(fake.calls().len(), 1);
    Ok(())
}

#[tokio::test]
async fn trace_repo_update_completed_generates_valid_sql() -> Result<()> {
    let fake = FakeDatabend::new(|sql, _db| {
        assert!(sql.starts_with("UPDATE traces SET status = 'completed'"));
        assert!(sql.contains("duration_ms = 150"));
        assert!(sql.contains("input_tokens = 100"));
        assert!(sql.contains("output_tokens = 50"));
        assert!(sql.contains("total_cost = 0.005"));
        assert!(sql.contains("WHERE trace_id = 't-1'"));
        Ok(paged_rows(&[], None, None))
    });
    let repo = TraceRepo::new(fake.pool());
    repo.update_completed("t-1", 150, 100, 50, 0.005).await?;
    Ok(())
}

#[tokio::test]
async fn trace_repo_update_failed_generates_valid_sql() -> Result<()> {
    let fake = FakeDatabend::new(|sql, _db| {
        assert!(sql.starts_with("UPDATE traces SET status = 'failed'"));
        assert!(sql.contains("duration_ms = 200"));
        assert!(sql.contains("WHERE trace_id = 't-2'"));
        Ok(paged_rows(&[], None, None))
    });
    let repo = TraceRepo::new(fake.pool());
    repo.update_failed("t-2", 200).await?;
    Ok(())
}

#[tokio::test]
async fn trace_repo_load_and_list_by_session_generate_valid_sql() -> Result<()> {
    let fake = FakeDatabend::new(|sql, _db| {
        if sql.contains("WHERE trace_id = 't-1' LIMIT 1") {
            return Ok(bendclaw::storage::pool::QueryResponse {
                id: String::new(),
                state: "Succeeded".into(),
                error: None,
                data: vec![trace_row()],
                next_uri: None,
                final_uri: None,
                schema: Vec::new(),
            });
        }
        if sql.contains("WHERE session_id = 'sess-1'") {
            return Ok(bendclaw::storage::pool::QueryResponse {
                id: String::new(),
                state: "Succeeded".into(),
                error: None,
                data: vec![trace_row()],
                next_uri: None,
                final_uri: None,
                schema: Vec::new(),
            });
        }
        panic!("unexpected SQL: {sql}");
    });
    let repo = TraceRepo::new(fake.pool());

    let loaded = repo.load("t-1").await?.expect("trace should exist");
    assert_eq!(loaded.trace_id, "trace-1");

    let listed = repo.list_by_session("sess-1", 10).await?;
    assert_eq!(listed.len(), 1);
    Ok(())
}

#[tokio::test]
async fn trace_repo_count_filtered_generates_valid_sql() -> Result<()> {
    let fake = FakeDatabend::new(|sql, _db| {
        assert!(sql.starts_with("SELECT COUNT(*) FROM traces WHERE"));
        assert!(sql.contains("agent_id = 'a-1'"));
        assert!(sql.contains("status = 'completed'"));
        Ok(agg_row(&["42"]))
    });
    let repo = TraceRepo::new(fake.pool());
    let filter = TraceListFilter {
        agent_id: "a-1",
        session_id: None,
        run_id: None,
        user_id: None,
        status: Some("completed"),
        start_time: None,
        end_time: None,
    };
    let count = repo.count_filtered(&filter).await?;
    assert_eq!(count, 42);
    Ok(())
}

#[tokio::test]
async fn trace_repo_list_filtered_with_all_filters() -> Result<()> {
    let fake = FakeDatabend::new(|sql, _db| {
        assert!(sql.contains("agent_id = 'a-1'"));
        assert!(sql.contains("session_id = 's-1'"));
        assert!(sql.contains("run_id = 'r-1'"));
        assert!(sql.contains("user_id = 'u-1'"));
        assert!(sql.contains("status = 'completed'"));
        assert!(sql.contains("created_at >= '2026-01-01'"));
        assert!(sql.contains("created_at <= '2026-12-31'"));
        assert!(sql.contains("ORDER BY created_at DESC"));
        assert!(sql.contains("LIMIT 10 OFFSET 5"));
        Ok(bendclaw::storage::pool::QueryResponse {
            id: String::new(),
            state: "Succeeded".into(),
            error: None,
            data: vec![trace_row()],
            next_uri: None,
            final_uri: None,
            schema: Vec::new(),
        })
    });
    let repo = TraceRepo::new(fake.pool());
    let filter = TraceListFilter {
        agent_id: "a-1",
        session_id: Some("s-1"),
        run_id: Some("r-1"),
        user_id: Some("u-1"),
        status: Some("completed"),
        start_time: Some("2026-01-01"),
        end_time: Some("2026-12-31"),
    };
    let rows = repo.list_filtered(&filter, "DESC", 10, 5).await?;
    assert_eq!(rows.len(), 1);
    Ok(())
}

#[tokio::test]
async fn trace_repo_summary_for_agent_generates_valid_sql() -> Result<()> {
    let call_count = std::sync::Arc::new(std::sync::Mutex::new(0u32));
    let cc = call_count.clone();
    let fake = FakeDatabend::new(move |sql, _db| {
        let mut n = cc.lock().unwrap();
        *n += 1;
        if *n == 1 {
            assert!(sql.contains("COUNT(*)"));
            assert!(sql.contains("SUM(input_tokens)"));
            assert!(sql.contains("agent_id = 'a-1'"));
            return Ok(agg_row(&[
                "10",
                "500",
                "300",
                "0.05",
                "120.5",
                "2026-03-11",
            ]));
        }
        // second query: span breakdown
        assert!(sql.contains("FROM spans"));
        assert!(sql.contains("agent_id = 'a-1'"));
        Ok(agg_row(&["5", "3", "1", "2"]))
    });
    let repo = TraceRepo::new(fake.pool());
    let summary = repo.summary_for_agent("a-1").await?;
    assert_eq!(summary.trace_count, 10);
    assert_eq!(summary.llm_calls, 5);
    Ok(())
}

#[tokio::test]
async fn trace_repo_agent_details_generates_valid_sql() -> Result<()> {
    let call_count = std::sync::Arc::new(std::sync::Mutex::new(0u32));
    let cc = call_count.clone();
    let fake = FakeDatabend::new(move |sql, _db| {
        let mut n = cc.lock().unwrap();
        *n += 1;
        match *n {
            1 => {
                // summary_for_agent q1
                Ok(agg_row(&["5", "200", "100", "0.02", "80.0", "2026-03-11"]))
            }
            2 => {
                // summary_for_agent q2 (span counts)
                Ok(agg_row(&["3", "2", "1", "0"]))
            }
            3 => {
                // trace_ids query
                assert!(sql.contains("SELECT trace_id FROM traces"));
                assert!(sql.contains("LIMIT 20"));
                Ok(paged_rows(&[&["trace-1"]], None, None))
            }
            4..=7 => {
                // breakdown queries (llm, tool, skill, errors)
                assert!(sql.contains("FROM spans"));
                assert!(sql.contains("GROUP BY name"));
                Ok(paged_rows(&[], None, None))
            }
            _ => panic!("unexpected call #{n}: {sql}"),
        }
    });
    let repo = TraceRepo::new(fake.pool());
    let details = repo.agent_details("a-1").await?;
    assert_eq!(details.trace_count, 5);
    assert_eq!(details.recent_trace_ids, vec!["trace-1"]);
    Ok(())
}

#[tokio::test]
async fn span_repo_append_and_list_generate_valid_sql() -> Result<()> {
    let fake = FakeDatabend::new(|sql, _db| {
        if sql.starts_with("INSERT INTO spans") {
            assert!(sql.contains("span_id"));
            assert!(sql.contains("NOW()"));
            return Ok(paged_rows(&[], None, None));
        }
        if sql.contains("WHERE trace_id = 'trace-1'") {
            return Ok(bendclaw::storage::pool::QueryResponse {
                id: String::new(),
                state: "Succeeded".into(),
                error: None,
                data: vec![span_row()],
                next_uri: None,
                final_uri: None,
                schema: Vec::new(),
            });
        }
        panic!("unexpected SQL: {sql}");
    });
    let span_repo = SpanRepo::new(fake.pool());
    let record = bendclaw::storage::SpanRecord {
        span_id: "sp-1".into(),
        trace_id: "trace-1".into(),
        parent_span_id: String::new(),
        name: "gpt-4o".into(),
        kind: "llm".into(),
        model_role: "main".into(),
        status: "completed".into(),
        duration_ms: 120,
        ttft_ms: 80,
        input_tokens: 100,
        output_tokens: 50,
        reasoning_tokens: 0,
        cost: 0.003,
        error_code: String::new(),
        error_message: String::new(),
        summary: "summary".into(),
        meta: "{}".into(),
        created_at: String::new(),
    };
    span_repo.append(&record).await?;
    let spans = span_repo.list_by_trace("trace-1").await?;
    assert_eq!(spans.len(), 1);
    assert_eq!(spans[0].span_id, "span-1");
    Ok(())
}
