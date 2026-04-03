use std::sync::Arc;
use std::sync::Mutex;

use async_trait::async_trait;
use bendclaw::memory::MemoryEntry;
use bendclaw::memory::MemoryScope;
use bendclaw::memory::MemorySearchResult;
use bendclaw::memory::MemoryService;
use bendclaw::memory::MemoryStore;
use bendclaw::types::Result;
use tokio_util::sync::CancellationToken;

// ── Fake MemoryStore ──

#[derive(Default, Clone)]
struct FakeStore {
    entries: Arc<Mutex<Vec<MemoryEntry>>>,
    touch_count: Arc<Mutex<u32>>,
}

#[async_trait]
impl MemoryStore for FakeStore {
    async fn write(&self, entry: &MemoryEntry) -> Result<()> {
        self.entries.lock().unwrap().push(entry.clone());
        Ok(())
    }
    async fn search(
        &self,
        query: &str,
        _user_id: &str,
        _agent_id: &str,
        limit: u32,
    ) -> Result<Vec<MemorySearchResult>> {
        let entries = self.entries.lock().unwrap();
        let results = entries
            .iter()
            .filter(|e| e.content.contains(query) || e.key.contains(query))
            .take(limit as usize)
            .map(|e| MemorySearchResult {
                id: e.id.clone(),
                key: e.key.clone(),
                content: e.content.clone(),
                scope: e.scope,
                agent_id: e.agent_id.clone(),
                score: 1.0,
                access_count: e.access_count,
                updated_at: e.updated_at.clone(),
            })
            .collect();
        Ok(results)
    }
    async fn get(&self, _u: &str, _a: &str, key: &str) -> Result<Option<MemoryEntry>> {
        let entries = self.entries.lock().unwrap();
        Ok(entries.iter().find(|e| e.key == key).cloned())
    }
    async fn get_by_id(&self, _u: &str, id: &str) -> Result<Option<MemoryEntry>> {
        let entries = self.entries.lock().unwrap();
        Ok(entries.iter().find(|e| e.id == id).cloned())
    }
    async fn delete(&self, _u: &str, id: &str) -> Result<()> {
        self.entries.lock().unwrap().retain(|e| e.id != id);
        Ok(())
    }
    async fn list(&self, _u: &str, _a: &str, limit: u32) -> Result<Vec<MemoryEntry>> {
        let entries = self.entries.lock().unwrap();
        Ok(entries.iter().take(limit as usize).cloned().collect())
    }
    async fn touch(&self, _u: &str, _id: &str) -> Result<()> {
        *self.touch_count.lock().unwrap() += 1;
        Ok(())
    }
    async fn prune(&self, _u: &str, _d: u32, _m: u32) -> Result<usize> {
        Ok(0)
    }
}

// ── Fake LLM ──

struct NoOpLlm;

#[async_trait]
impl bendclaw::llm::provider::LLMProvider for NoOpLlm {
    async fn chat(
        &self,
        _m: &str,
        _msgs: &[bendclaw::llm::message::ChatMessage],
        _t: &[bendclaw::llm::tool::ToolSchema],
        _temp: f64,
    ) -> Result<bendclaw::llm::provider::LLMResponse> {
        Ok(bendclaw::llm::provider::LLMResponse {
            content: Some("[]".into()),
            tool_calls: vec![],
            finish_reason: Some("stop".into()),
            usage: None,
            model: None,
        })
    }
    fn chat_stream(
        &self,
        _m: &str,
        _msgs: &[bendclaw::llm::message::ChatMessage],
        _t: &[bendclaw::llm::tool::ToolSchema],
        _temp: f64,
    ) -> bendclaw::llm::stream::ResponseStream {
        bendclaw::llm::stream::ResponseStream::from_error(bendclaw::types::ErrorCode::internal(
            "not implemented",
        ))
    }
}

fn make_service() -> (MemoryService, FakeStore) {
    let store = FakeStore::default();
    let llm: Arc<dyn bendclaw::llm::provider::LLMProvider> = Arc::new(NoOpLlm);
    let svc = MemoryService::new(Arc::new(store.clone()), llm, "test".into());
    (svc, store)
}

fn make_entry(key: &str, content: &str) -> MemoryEntry {
    MemoryEntry {
        id: key.into(),
        user_id: "u1".into(),
        agent_id: "a1".into(),
        scope: MemoryScope::Agent,
        key: key.into(),
        content: content.into(),
        access_count: 0,
        last_accessed_at: String::new(),
        created_at: String::new(),
        updated_at: String::new(),
    }
}

// ── Tests ──

#[tokio::test]
async fn save_writes_to_store() {
    let (svc, store) = make_service();
    svc.save("u1", "a1", "timezone", "UTC+8", MemoryScope::Shared)
        .await
        .unwrap();
    let entries = store.entries.lock().unwrap();
    assert_eq!(entries.len(), 1);
    assert_eq!(entries[0].key, "timezone");
    assert_eq!(entries[0].content, "UTC+8");
    assert_eq!(entries[0].scope, MemoryScope::Shared);
    assert_eq!(entries[0].agent_id, "a1");
}

#[tokio::test]
async fn save_default_scope_is_agent() {
    let (svc, store) = make_service();
    svc.save("u1", "a1", "fact", "some fact", MemoryScope::Agent)
        .await
        .unwrap();
    let entries = store.entries.lock().unwrap();
    assert_eq!(entries[0].scope, MemoryScope::Agent);
}

#[tokio::test]
async fn recall_returns_entries() {
    let (svc, store) = make_service();
    store
        .entries
        .lock()
        .unwrap()
        .push(make_entry("pref", "likes rust"));
    let entries = svc.recall("u1", "a1", 10).await;
    assert_eq!(entries.len(), 1);
    assert_eq!(entries[0].key, "pref");
}

#[tokio::test]
async fn recall_empty_store() {
    let (svc, _store) = make_service();
    let entries = svc.recall("u1", "a1", 10).await;
    assert!(entries.is_empty());
}

#[tokio::test]
async fn recall_respects_limit() {
    let (svc, store) = make_service();
    for i in 0..10 {
        store
            .entries
            .lock()
            .unwrap()
            .push(make_entry(&format!("k{i}"), &format!("fact {i}")));
    }
    let entries = svc.recall("u1", "a1", 3).await;
    assert_eq!(entries.len(), 3);
}

#[tokio::test]
async fn search_returns_matching() {
    let (svc, store) = make_service();
    store
        .entries
        .lock()
        .unwrap()
        .push(make_entry("lang", "prefers rust over go"));
    let results = svc.search("rust", "u1", "a1", 5).await.unwrap();
    assert_eq!(results.len(), 1);
    assert_eq!(results[0].key, "lang");
}

#[tokio::test]
async fn search_touches_results() {
    let (svc, store) = make_service();
    store.entries.lock().unwrap().push(make_entry("k", "match"));
    svc.search("match", "u1", "a1", 5).await.unwrap();
    assert_eq!(*store.touch_count.lock().unwrap(), 1);
}

#[tokio::test]
async fn extract_empty_transcript() {
    let (svc, store) = make_service();
    let cancel = CancellationToken::new();
    let result = svc.extract_and_save("", "u1", "a1", cancel).await;
    assert_eq!(result.facts_written, 0);
    assert!(store.entries.lock().unwrap().is_empty());
}

#[tokio::test]
async fn extract_respects_cancellation() {
    let (svc, store) = make_service();
    let cancel = CancellationToken::new();
    cancel.cancel();
    let result = svc
        .extract_and_save("some conversation text", "u1", "a1", cancel)
        .await;
    assert_eq!(result.facts_written, 0);
    assert!(store.entries.lock().unwrap().is_empty());
}

#[tokio::test]
async fn extract_noop_llm_returns_zero() {
    let (svc, store) = make_service();
    let cancel = CancellationToken::new();
    let result = svc
        .extract_and_save("user said they prefer dark mode", "u1", "a1", cancel)
        .await;
    assert_eq!(result.facts_written, 0);
    assert!(store.entries.lock().unwrap().is_empty());
}

#[tokio::test]
async fn run_hygiene() {
    let (svc, _store) = make_service();
    let pruned = svc.run_hygiene("u1", 30, 2).await.unwrap();
    assert_eq!(pruned, 0);
}
