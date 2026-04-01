use bendclaw::kernel::session::assembly::cloud::CloudAssembler;
use bendclaw::kernel::session::assembly::cloud::CloudBuildOptions;
use bendclaw::kernel::session::assembly::contract::SessionOwner;
use bendclaw::storage::pool::QueryResponse;

use crate::common::fake_databend::FakeDatabend;
use crate::common::test_runtime::test_runtime;

fn noop_fake() -> FakeDatabend {
    FakeDatabend::new(|_sql, _database| {
        Ok(QueryResponse {
            id: String::new(),
            state: "Succeeded".to_string(),
            error: None,
            data: Vec::new(),
            next_uri: None,
            final_uri: None,
            schema: Vec::new(),
        })
    })
}

#[tokio::test]
async fn cloud_assembly_labels_reflect_owner() {
    let runtime = test_runtime(noop_fake());
    let assembler = CloudAssembler { runtime };
    let owner = SessionOwner {
        agent_id: "agent-1".to_string(),
        user_id: "user-1".to_string(),
    };
    let assembly = assembler
        .assemble("session-1", &owner, CloudBuildOptions::default())
        .await
        .expect("assemble");
    assert_eq!(assembly.labels.agent_id.as_ref(), "agent-1");
    assert_eq!(assembly.labels.user_id.as_ref(), "user-1");
    assert_eq!(assembly.labels.session_id.as_ref(), "session-1");
}

#[tokio::test]
async fn cloud_assembly_has_no_cluster_by_default() {
    let runtime = test_runtime(noop_fake());
    let assembler = CloudAssembler { runtime };
    let owner = SessionOwner {
        agent_id: "agent-2".to_string(),
        user_id: "user-2".to_string(),
    };
    let assembly = assembler
        .assemble("session-2", &owner, CloudBuildOptions::default())
        .await
        .expect("assemble");
    assert!(assembly.agent.cluster_client.is_none());
    assert!(assembly.agent.directive.is_none());
}

#[tokio::test]
async fn cloud_assembly_prompt_config_none_without_db_record() {
    let runtime = test_runtime(noop_fake());
    let assembler = CloudAssembler { runtime };
    let owner = SessionOwner {
        agent_id: "agent-3".to_string(),
        user_id: "user-3".to_string(),
    };
    let assembly = assembler
        .assemble("session-3", &owner, CloudBuildOptions::default())
        .await
        .expect("assemble");
    // No config record in DB → prompt_config is None
    assert!(assembly.agent.prompt_config.is_none());
}
