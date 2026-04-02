use bendclaw::kernel::run::execution::compaction::rules::keep_budget;
use bendclaw::kernel::run::execution::compaction::rules::plan_compaction_split;
use bendclaw::kernel::run::execution::compaction::rules::SUMMARY_RESERVE;
use bendclaw::kernel::Message;

#[test]
fn keep_budget_reserves_summary_space() {
    assert_eq!(keep_budget(10_000), 10_000 - SUMMARY_RESERVE);
    assert!(keep_budget(100_000) < 100_000);
}

#[test]
fn plan_compaction_split_preserves_tool_result_pairing() {
    let messages = vec![
        Message::user("old user"),
        Message::assistant("old assistant"),
        Message::assistant("tool context"),
        Message::tool_result("tc-1", "shell", "tool output", true),
        Message::user("latest"),
    ];
    let msg_tokens = vec![8_000, 8_000, 1_000, 500, 100];

    let plan = plan_compaction_split(&messages, &msg_tokens, 6_000).expect("split plan");

    assert_eq!(plan.split_index, 2);
    assert!(plan.kept_tokens >= 1_600);
}

#[test]
fn plan_compaction_split_ignores_system_and_compaction_messages() {
    let messages = vec![
        Message::system("system"),
        Message::compaction("older summary"),
        Message::user("recent one"),
        Message::assistant("recent two"),
    ];
    let msg_tokens = vec![100, 100, 200, 200];

    let plan = plan_compaction_split(&messages, &msg_tokens, 10_000).expect("split plan");

    assert_eq!(plan.split_index, 2);
}
