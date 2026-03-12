use std::sync::Arc;
use std::time::Duration;

use bendclaw::client::BendclawClient;
use bendclaw::client::ClusterClient;
use bendclaw::kernel::cluster::ClusterService;
use bendclaw::kernel::tools::registry::register_cluster_tools;
use bendclaw::kernel::tools::registry::ToolRegistry;
use bendclaw::kernel::tools::ToolId;

fn make_service() -> Arc<ClusterService> {
    let cc = Arc::new(ClusterClient::new(
        "https://fake.evot.ai",
        "fake-token",
        "node-1",
        "http://127.0.0.1:8787",
    ));
    let bc = Arc::new(BendclawClient::new("fake-key", Duration::from_secs(5)));
    Arc::new(ClusterService::new(cc, bc))
}

#[test]
fn dispatch_table_list_empty_initially() {
    let svc = make_service();
    let dt = svc.create_dispatch_table();
    assert!(dt.list().is_empty());
}

#[test]
fn dispatch_table_get_unknown_returns_none() {
    let svc = make_service();
    let dt = svc.create_dispatch_table();
    assert!(dt.get("nonexistent").is_none());
}

#[test]
fn cluster_tools_registered_when_config_present() {
    let svc = make_service();
    let dt = svc.create_dispatch_table();

    let mut registry = ToolRegistry::new();
    register_cluster_tools(&mut registry, svc, dt);

    let names = registry.list();
    assert!(names.contains(&"cluster_nodes"));
    assert!(names.contains(&"cluster_dispatch"));
    assert!(names.contains(&"cluster_collect"));
    assert_eq!(names.len(), 3);
}

#[test]
fn no_cluster_tools_without_registration() {
    let registry = ToolRegistry::new();
    assert!(registry.get(ToolId::ClusterNodes.as_str()).is_none());
    assert!(registry.get(ToolId::ClusterDispatch.as_str()).is_none());
    assert!(registry.get(ToolId::ClusterCollect.as_str()).is_none());
}

#[test]
fn cluster_tool_schemas_valid() {
    let svc = make_service();
    let dt = svc.create_dispatch_table();

    let mut registry = ToolRegistry::new();
    register_cluster_tools(&mut registry, svc, dt);

    let schemas = registry.tool_schemas();
    assert_eq!(schemas.len(), 3);

    for schema in &schemas {
        assert_eq!(schema.schema_type, "function");
        assert!(!schema.function.name.is_empty());
        assert!(!schema.function.description.is_empty());
        assert_eq!(
            schema
                .function
                .parameters
                .get("type")
                .and_then(|v| v.as_str()),
            Some("object")
        );
    }
}

#[test]
fn cluster_dispatch_tool_requires_node_id() {
    let svc = make_service();
    let dt = svc.create_dispatch_table();

    let mut registry = ToolRegistry::new();
    register_cluster_tools(&mut registry, svc, dt);

    let tool = registry.get(ToolId::ClusterDispatch.as_str()).unwrap();
    let schema = tool.parameters_schema();
    let required = schema
        .get("required")
        .and_then(|v| v.as_array())
        .expect("should have required array");
    let required_strs: Vec<&str> = required.iter().filter_map(|v| v.as_str()).collect();
    assert!(required_strs.contains(&"node_id"));
    assert!(required_strs.contains(&"agent_id"));
    assert!(required_strs.contains(&"task"));
    assert!(!required_strs.contains(&"endpoint"));
}

#[test]
fn resolve_endpoint_unknown_node_fails() {
    let svc = make_service();
    let result = svc.resolve_endpoint("unknown-node");
    assert!(result.is_err());
}
