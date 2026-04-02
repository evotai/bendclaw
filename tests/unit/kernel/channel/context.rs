use bendclaw::kernel::channels::model::context::ChannelContext;

#[test]
fn parse_feishu_session() {
    let ctx =
        ChannelContext::from_base_key("feishu:01kkzesy9x1b0x4rt4j57mzevw:oc_6406fca8d3a2").unwrap();
    assert_eq!(ctx.channel_type, "feishu");
    assert_eq!(ctx.account_id, "01kkzesy9x1b0x4rt4j57mzevw");
    assert_eq!(ctx.chat_id, "oc_6406fca8d3a2");
}

#[test]
fn parse_telegram_session() {
    let ctx = ChannelContext::from_base_key("telegram:bot123:chat456").unwrap();
    assert_eq!(ctx.channel_type, "telegram");
    assert_eq!(ctx.account_id, "bot123");
    assert_eq!(ctx.chat_id, "chat456");
}

#[test]
fn parse_github_session() {
    let ctx = ChannelContext::from_base_key("github:app42:issue_789").unwrap();
    assert_eq!(ctx.channel_type, "github");
    assert_eq!(ctx.account_id, "app42");
    assert_eq!(ctx.chat_id, "issue_789");
}

#[test]
fn parse_http_api_session() {
    let ctx = ChannelContext::from_base_key("http_api:acc1:conv99").unwrap();
    assert_eq!(ctx.channel_type, "http_api");
    assert_eq!(ctx.account_id, "acc1");
    assert_eq!(ctx.chat_id, "conv99");
}

#[test]
fn chat_id_with_colons_preserved() {
    let ctx = ChannelContext::from_base_key("feishu:acc:chat:with:colons").unwrap();
    assert_eq!(ctx.channel_type, "feishu");
    assert_eq!(ctx.account_id, "acc");
    assert_eq!(ctx.chat_id, "chat:with:colons");
}

#[test]
fn non_channel_session_returns_none() {
    assert!(ChannelContext::from_base_key("s1").is_none());
    assert!(ChannelContext::from_base_key("").is_none());
    assert!(ChannelContext::from_base_key("only:two").is_none());
}

#[test]
fn empty_parts_return_none() {
    assert!(ChannelContext::from_base_key("::").is_none());
    assert!(ChannelContext::from_base_key("feishu::chat").is_none());
    assert!(ChannelContext::from_base_key("feishu:acc:").is_none());
}
