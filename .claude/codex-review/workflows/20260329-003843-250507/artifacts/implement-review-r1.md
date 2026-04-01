## Implementation Review

### Plan Adherence

整体上，4 个计划项都能看到对应实现，但存在关键偏差和实现缺口。  
另外说明：我尝试过 `git diff`，但当前沙箱因临时文件权限限制导致 diff 失败，因此本次审查基于当前代码直接逐文件核对。

- [Deviation] `list_user_ids()` 已新增并接入 sync loop（计划 Issue 1）— justified  
- [Deviation] 技能 HTTP handler 已统一改为 `ctx.user_id`（计划 Issue 2）— justified  
- [Deviation] `touch_last_used` 签名和 SQL 已改，但技能执行链路没有实际调用 `touch_used`（计划 Issue 3）— unjustified  
- [Deviation] 订阅读路径已接入变量与技能（计划 Issue 4），但技能镜像实现丢失 owner 维度，导致名称冲突覆盖 — unjustified  
- [Deviation] 额外引入 `OrgServices`/`SubscriptionStore` 聚合层，不在原计划文本内，但方向上改善职责分层 — justified

### Findings

[CRITICAL-1] File: [runtime_builder.rs](/Users/bohu/github/evotai/bendclaw/src/kernel/runtime/runtime_builder.rs:390), [sync.rs](/Users/bohu/github/evotai/bendclaw/src/kernel/skills/remote/sync.rs:70), [sync.rs](/Users/bohu/github/evotai/bendclaw/src/kernel/skills/remote/sync.rs:23)  
`list_user_ids()` 失败时被降级为 `Vec::new()` 继续执行 sync；而 `sync()` 无论拉取是否完整都会执行 `evict_stale()`。这会在瞬时 DB 异常或部分用户拉取失败时误删本地镜像（全量或部分）。  
建议：  
1. `list_user_ids` 失败时直接跳过本轮 sync（不要调用 `sync`）。  
2. `sync` 内任一用户拉取失败时，至少跳过该用户的 eviction，或整轮 fail-fast，不做 destructive eviction。  
3. 记录明确错误日志，不要静默退化为空集。

[CRITICAL-2] File: [sync.rs](/Users/bohu/github/evotai/bendclaw/src/kernel/skills/remote/sync.rs:38), [sync.rs](/Users/bohu/github/evotai/bendclaw/src/kernel/skills/remote/sync.rs:54), [sync.rs](/Users/bohu/github/evotai/bendclaw/src/kernel/skills/remote/sync.rs:67)  
订阅技能镜像时把 `skill.user_id` 重写为订阅者，并且磁盘 key 仅用 `(user_id, skill_name)`。当“自有技能名”与“订阅技能名”冲突时，后写入会覆盖前写入，破坏 ownership 边界。  
建议：  
1. 镜像键必须包含 owner 维度（如 `(subscriber, owner, skill_name)`）。  
2. 目录结构改为可区分 owner（例如 `.remote/subscribed/{owner}/{name}`）。  
3. `SkillCatalog/resolve` 明确优先级：own > subscribed > hub，并保持 deterministic。

[HIGH-1] File: [store.rs](/Users/bohu/github/evotai/bendclaw/src/kernel/variables/store.rs:154), [workspace.rs](/Users/bohu/github/evotai/bendclaw/src/kernel/session/workspace.rs:138)  
变量读路径接入订阅后，`list_active` 返回“自有+订阅”变量；`Workspace::from_variables` 再按 `key` 直接收敛到 `HashMap`。同名 key 时会发生隐式覆盖，且当前 `ORDER BY created_at DESC` + `collect` 组合无法保证“自有优先”。  
建议：  
1. 在 service 层先按 key 去重并显式优先级（own > subscribed）。  
2. 或 SQL 层用窗口函数/优先级列做 deterministic 选择。  
3. 加冲突监控日志，避免静默覆盖。

[MEDIUM-1] File: [service.rs](/Users/bohu/github/evotai/bendclaw/src/kernel/skills/service.rs:117), [runner.rs](/Users/bohu/github/evotai/bendclaw/src/kernel/skills/runner.rs:144)  
计划 Issue 3 的“touch_last_used 链路”未完全落地：`SkillService::touch_used()` 已存在，但执行路径没有调用，导致技能 `last_used_by` 实际不会更新。  
建议：在 `SkillRunner` 执行后（至少成功分支）调用 `self.skills.touch_used(self.user_id.clone(), skill.name.clone(), self.agent_id.clone())`。

[MEDIUM-2] File: [http.rs](/Users/bohu/github/evotai/bendclaw/src/service/v1/skills/http.rs:85)  
API 路由仍是 `/agents/{agent_id}/skills`，但 handler 全部忽略 `agent_id`。从架构与职责清晰度看，资源语义已是 user-scoped，却保留 agent-scoped 路径，造成边界认知混乱。  
建议：不考虑兼容性时应改为 user-scoped 路由；若保留路径则至少校验 agent 与 user 归属关系，避免“路径语义”和“实际鉴权语义”分裂。

[LOW-1] File: [search.rs](/Users/bohu/github/evotai/bendclaw/src/kernel/tools/builtins/web/search.rs:297)  
`web_search` 触发变量 usage touch 时仍写入空 actor（`""`），会污染 `last_used_by` 可观测性数据。  
建议：统一使用 `ctx.agent_id` 或 `ctx.user_id`。

### Test Coverage

当前有 CRUD 与基础 runner 覆盖，但对本次计划最关键的“订阅读路径+镜像一致性+冲突处理”覆盖不足。且我无法在该只读沙箱实际执行测试命令验证通过情况。

缺失用例：
- [ ] Error case: `list_user_ids` 失败时不得触发 destructive eviction  
- [ ] Error case: 单用户 `store.list()`/`list_subscribed()` 失败时不应删该用户已有镜像  
- [ ] Integration: 自有技能与订阅技能同名时，own/subscribed 优先级与隔离行为  
- [ ] Edge case: 两个订阅来源同名技能的确定性选择与隔离  
- [ ] Edge case: 变量同 key（自有 vs 订阅）时的优先级与最终注入 env  
- [ ] Regression: skill `touch_last_used` 在 runner 执行后真实更新（含 user_id 过滤）

### Summary

- Critical: 2
- High: 1
- Medium: 2
- Low: 1
- Plan deviations: 5 (justified: 3, unjustified: 2)

Overall: 主体重构方向（按 user 边界）是对的，但 sync/镜像层在错误处理与 owner 维度建模上有严重缺陷，当前不建议合入主干。