# 高优先级改进方案：借鉴 yoagent 的三项改进

基于 yolo/yoagent 与 bendclaw 的对比分析，以下三项改进优先级最高、投入产出比最好。

---

## 改进一：Context Overflow 检测 + 自动 Compaction 重试

### 问题
bendclaw 目前只有**主动**的 context pressure 检测（LLM 调用前按 token 估算），没有**被动**检测——当 LLM 返回 context overflow 错误时，直接当作普通 `LLM_REQUEST` 错误处理，run 终止。yoagent 有 15+ 种 provider 的 overflow 短语检测，能自动识别并触发 compaction 重试。

### 方案

#### Step 1: 新增 ErrorCode `LLM_CONTEXT_OVERFLOW` (1205)
- 文件: `src/base/error.rs`
- 在 `LLM_PARSE(1204)` 后新增 `LLM_CONTEXT_OVERFLOW(1205)`

#### Step 2: 新增 overflow 检测函数
- 文件: `src/llm/providers/common.rs`（新建或在现有 common 模块中）
- 实现 `is_context_overflow(status: u16, message: &str) -> bool`
- 移植 yoagent 的 15+ 检测短语（case-insensitive）：
  - "prompt is too long" (Anthropic)
  - "exceeds the context window" (OpenAI)
  - "input is too long" (Bedrock)
  - "exceeds the maximum" (Google)
  - "maximum prompt length" (xAI)
  - "reduce the length of the messages" (Groq)
  - "maximum context length" (OpenRouter)
  - "context length exceeded" (generic)
  - "context_length_exceeded" (generic)
  - "too many tokens" (generic)
  - "token limit exceeded" (generic)
  - 等等

#### Step 3: 在 provider 错误分类中使用
- 文件: `src/llm/providers/openai.rs`, `src/llm/providers/anthropic.rs`
- 在 HTTP 错误处理中（status 400/413），调用 `is_context_overflow()` 检测
- 匹配时返回 `ErrorCode::llm_context_overflow()` 而非 `ErrorCode::llm_request()`

#### Step 4: 新增 TurnTransition::CompactAndRetry
- 文件: `src/kernel/run/transition.rs`
- 新增变体: `CompactAndRetry`
- 在 `apply_turn_result()` 中，当 LLM 错误为 context overflow 时返回此变体

#### Step 5: 在 run loop 中处理 CompactAndRetry
- 文件: `src/kernel/run/engine/engine_run.rs`
- 在 `step()` 返回 `CompactAndRetry` 时：
  1. 强制触发 compaction（忽略 pressure level，直接调用 `Compactor::compact()`）
  2. 重试 LLM 调用
  3. 最多重试 2 次，防止无限循环
  4. 如果 compaction 后仍然 overflow，返回 `TurnTransition::Error`

#### Step 6: circuit breaker 排除 context overflow
- 文件: `src/llm/circuit_breaker.rs`
- `is_transient()` 中将 `LLM_CONTEXT_OVERFLOW` 标记为非 transient（不应触发熔断）

### 涉及文件
| 文件 | 变更 |
|------|------|
| `src/base/error.rs` | 新增 error code |
| `src/llm/providers/common.rs` | 新增检测函数 |
| `src/llm/providers/openai.rs` | 使用检测函数 |
| `src/llm/providers/anthropic.rs` | 使用检测函数 |
| `src/kernel/run/transition.rs` | 新增 CompactAndRetry |
| `src/kernel/run/engine/engine_run.rs` | 处理 CompactAndRetry |
| `src/llm/circuit_breaker.rs` | 排除 context overflow |
| `tests/unit/` | 新增测试 |

---

## 改进二：OpenAI-Compatible Provider + Compat Flags

### 问题
bendclaw 只有 OpenAI 和 Anthropic 两个 provider。要支持 xAI、Groq、DeepSeek、Mistral 等 OpenAI 兼容服务，需要为每个写独立实现。yoagent 用一个 `OpenAiCompat` flags 结构体 + 一个通用 provider 覆盖了 10+ 服务。

### 方案

#### Step 1: 定义 compat 类型
- 文件: `src/llm/compat.rs`（新建）
- 定义：

```rust
pub enum MaxTokensField {
    MaxTokens,
    MaxCompletionTokens,
}

pub enum ThinkingFormat {
    None,
    OpenAi,    // reasoning_content 字段
    Xai,       // reasoning 字段
}

pub struct OpenAiCompat {
    pub supports_developer_role: bool,
    pub supports_usage_in_streaming: bool,
    pub supports_reasoning_effort: bool,
    pub max_tokens_field: MaxTokensField,
    pub requires_tool_result_name: bool,
    pub thinking_format: ThinkingFormat,
}
```

- 提供 preset 构造函数：`OpenAiCompat::openai()`, `::groq()`, `::deepseek()`, `::xai()`, `::mistral()` 等

#### Step 2: 配置层集成
- 文件: `src/llm/config.rs`
- `ProviderEndpoint` 新增 `compat: Option<OpenAiCompat>` 字段
- 支持从 TOML 配置反序列化
- 支持 `provider = "openai-compat"` 类型 + preset 名称

#### Step 3: Registry 层适配
- 文件: `src/llm/registry.rs`
- 修改 factory 签名，传递 compat flags
- 注册 `"openai-compat"` provider 类型

#### Step 4: 修改 OpenAI Provider 使用 compat flags
- 文件: `src/llm/providers/openai.rs`
- `OpenAIProvider` 新增 `compat: OpenAiCompat` 字段
- `build_body()` 中根据 flags 条件构建：
  - `max_tokens` vs `max_completion_tokens`
  - `system` vs `developer` role
  - `stream_options.include_usage` 是否启用
  - tool result 是否包含 `name` 字段
  - `reasoning_effort` 参数
- 流式响应解析中根据 `thinking_format` 提取 reasoning 内容

#### Step 5: 配置示例
```toml
[[llm.providers]]
name = "deepseek"
provider = "openai-compat"
base_url = "https://api.deepseek.com/v1"
api_key = "sk-..."
model = "deepseek-chat"
weight = 50
compat = "deepseek"  # 使用 preset

[[llm.providers]]
name = "custom"
provider = "openai-compat"
base_url = "https://my-llm.example.com/v1"
api_key = "..."
model = "my-model"
[llm.providers.compat]
supports_developer_role = false
max_tokens_field = "max_tokens"
supports_usage_in_streaming = true
```

### 涉及文件
| 文件 | 变更 |
|------|------|
| `src/llm/compat.rs` | 新建，定义 compat 类型和 presets |
| `src/llm/config.rs` | ProviderEndpoint 新增 compat 字段 |
| `src/llm/registry.rs` | 更新 factory 签名 |
| `src/llm/providers/openai.rs` | 使用 compat flags 构建请求 |
| `src/llm/mod.rs` | 导出 compat 模块 |
| `tests/unit/` | 新增测试 |

---

## 改进三：可插拔 Tool 执行策略

### 问题
bendclaw 的 tool dispatch 固定为并行执行（`join_bounded`），没有策略选择。某些场景需要顺序执行（工具间有隐式依赖）或分批执行（需要中途检查 steering）。yoagent 支持 Sequential / Parallel / Batched 三种策略，且在每种策略的合适时机检查 steering。

### 方案

#### Step 1: 定义策略枚举
- 文件: `src/kernel/run/dispatcher.rs`

```rust
pub enum ToolExecutionStrategy {
    /// 所有工具并行执行，执行完后检查 steering（当前行为）
    Parallel,
    /// 逐个执行，每个工具后检查 steering
    Sequential,
    /// 分批执行，每批并行，批间检查 steering
    Batched { size: usize },
}
```

#### Step 2: 修改 ToolDispatcher
- 文件: `src/kernel/run/dispatcher.rs`
- `execute_calls()` 接受 `strategy` 和 `steering_source` 参数
- 实现三种执行路径：
  - **Parallel**: 保持现有 `join_bounded()` 逻辑
  - **Sequential**: 循环执行，每次执行后调用 `steering_source.check_steering()`，如果返回 Redirect 则跳过剩余工具
  - **Batched**: 用 `chunks(size)` 分批，每批用 `join_bounded()`，批间检查 steering

#### Step 3: 处理被跳过的工具
- 被 steering 中断跳过的工具，生成 `ToolCallResult` 标记为 skipped
- 返回 "Tool execution skipped due to steering redirect" 消息
- 确保 LLM 收到所有 tool_call_id 的结果（否则某些 provider 会报错）

#### Step 4: 在 Context 中存储策略
- 文件: `src/kernel/run/context.rs`
- `Context` 新增 `tool_execution_strategy: ToolExecutionStrategy` 字段
- 默认值: `Parallel`（保持向后兼容）

#### Step 5: 从配置初始化
- 文件: `src/kernel/session/` 相关文件
- Session 创建时从 agent 配置读取策略
- 传递到 Engine Context

#### Step 6: engine_tools 集成
- 文件: `src/kernel/run/engine/engine_tools.rs`
- `dispatch_tools()` 从 context 读取策略
- 将 `steering_source` 传递给 dispatcher
- Sequential/Batched 模式下，steering redirect 的消息注入到 context.messages

### 涉及文件
| 文件 | 变更 |
|------|------|
| `src/kernel/run/dispatcher.rs` | 新增策略枚举，实现三种执行路径 |
| `src/kernel/run/context.rs` | 新增策略字段 |
| `src/kernel/run/engine/engine_tools.rs` | 传递策略和 steering_source |
| `tests/unit/` | 新增测试 |

---

## 实施顺序建议

1. **Context Overflow 检测**（最高优先级，工作量最小）
   - 独立性强，不影响其他模块
   - 直接提升系统鲁棒性
   - 预计改动 ~200 行

2. **OpenAI-Compatible Provider**（高优先级，工作量中等）
   - 解锁多 provider 支持
   - 需要新建模块 + 修改配置层
   - 预计改动 ~400 行

3. **Tool 执行策略**（中高优先级，工作量中等）
   - 需要修改 dispatcher 核心逻辑
   - 需要 threading steering_source
   - 预计改动 ~300 行
