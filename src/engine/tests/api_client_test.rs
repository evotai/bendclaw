use std::collections::HashMap;
use std::error::Error;
use std::sync::Arc;

use bend_agent::ApiClient;
use bend_agent::ContentBlock;
use bend_agent::Message;
use bend_agent::MessageRole;
use bend_agent::ProviderKind;
use tokio::io::AsyncReadExt;
use tokio::io::AsyncWriteExt;
use tokio::net::TcpListener;
use tokio::sync::Mutex;

type TestResult = std::result::Result<(), Box<dyn Error>>;

struct TestServer {
    base_url: String,
    request_line: Arc<Mutex<Option<String>>>,
}

impl TestServer {
    async fn request_line(&self) -> Option<String> {
        self.request_line.lock().await.clone()
    }
}

async fn spawn_server(status_line: &str, body: String) -> Result<TestServer, Box<dyn Error>> {
    let listener = TcpListener::bind("127.0.0.1:0").await?;
    let address = listener.local_addr()?;
    let request_line = Arc::new(Mutex::new(None));
    let captured_request_line = request_line.clone();
    let status_line = status_line.to_string();

    tokio::spawn(async move {
        let accepted = listener.accept().await;
        let (mut stream, _) = match accepted {
            Ok(parts) => parts,
            Err(_) => return,
        };

        let mut request = Vec::new();
        let mut buffer = [0_u8; 4096];
        loop {
            let read = stream.read(&mut buffer).await;
            let read = match read {
                Ok(read) => read,
                Err(_) => return,
            };

            if read == 0 {
                break;
            }

            request.extend_from_slice(&buffer[..read]);
            if request.windows(4).any(|window| window == b"\r\n\r\n") {
                break;
            }
        }

        let request = String::from_utf8_lossy(&request).to_string();
        let first_line = request.lines().next().map(str::to_string);
        *captured_request_line.lock().await = first_line;

        let response = format!(
            "HTTP/1.1 {status_line}\r\ncontent-type: text/event-stream\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{body}",
            body.len()
        );
        let _ = stream.write_all(response.as_bytes()).await;
    });

    Ok(TestServer {
        base_url: format!("http://{address}"),
        request_line,
    })
}

fn user_message(text: &str) -> Message {
    Message {
        role: MessageRole::User,
        content: vec![ContentBlock::Text {
            text: text.to_string(),
        }],
    }
}

fn openai_success_stream() -> String {
    [
        r#"data: {"choices":[{"delta":{"content":"pong"}}]}"#,
        r#"data: {"choices":[{"delta":{},"finish_reason":"stop"}],"usage":{"prompt_tokens":1,"completion_tokens":1}}"#,
        "data: [DONE]",
    ]
    .join("\n")
}

fn anthropic_success_stream() -> String {
    [
        r#"data: {"type":"message_start","message":{"usage":{"input_tokens":1,"cache_creation_input_tokens":0,"cache_read_input_tokens":0}}}"#,
        r#"data: {"type":"content_block_start","index":0,"content_block":{"type":"text","text":""}}"#,
        r#"data: {"type":"content_block_delta","index":0,"delta":{"type":"text_delta","text":"pong"}}"#,
        r#"data: {"type":"content_block_stop","index":0}"#,
        r#"data: {"type":"message_delta","delta":{"stop_reason":"end_turn"},"usage":{"output_tokens":1}}"#,
        "data: [DONE]",
    ]
    .join("\n")
}

#[tokio::test]
async fn explicit_openai_provider_uses_openai_path() -> TestResult {
    let server = spawn_server("200 OK", openai_success_stream()).await?;
    let client = ApiClient::with_provider(
        ProviderKind::OpenAi,
        Some("test-key".to_string()),
        Some(server.base_url.clone()),
        Some("claude-sonnet-4-6-20250514".to_string()),
        HashMap::new(),
    )?;

    let response = client
        .create_message(&[user_message("ping")], None, None, Some(64), None)
        .await?;

    let request_line = server
        .request_line()
        .await
        .ok_or_else(|| std::io::Error::other("missing request line"))?;

    assert!(request_line.contains("/v1/chat/completions"));
    assert_eq!(response.message.role, MessageRole::Assistant);
    assert_eq!(response.usage.input_tokens, 1);
    assert_eq!(response.usage.output_tokens, 1);

    Ok(())
}

#[tokio::test]
async fn explicit_anthropic_provider_uses_anthropic_path() -> TestResult {
    let server = spawn_server("200 OK", anthropic_success_stream()).await?;
    let client = ApiClient::with_provider(
        ProviderKind::Anthropic,
        Some("test-key".to_string()),
        Some(server.base_url.clone()),
        Some("gpt-4o".to_string()),
        HashMap::new(),
    )?;

    let response = client
        .create_message(&[user_message("ping")], None, None, Some(64), None)
        .await?;

    let request_line = server
        .request_line()
        .await
        .ok_or_else(|| std::io::Error::other("missing request line"))?;

    assert!(request_line.contains("/v1/messages"));
    assert_eq!(response.message.role, MessageRole::Assistant);
    assert_eq!(response.usage.input_tokens, 1);
    assert_eq!(response.usage.output_tokens, 1);

    Ok(())
}

#[tokio::test]
async fn openai_stream_errors_surface_from_create_message() -> TestResult {
    let server = spawn_server(
        "200 OK",
        [
            r#"data: {"error":{"message":"openai stream failed"}}"#,
            "data: [DONE]",
        ]
        .join("\n"),
    )
    .await?;
    let client = ApiClient::with_provider(
        ProviderKind::OpenAi,
        Some("test-key".to_string()),
        Some(server.base_url),
        Some("gpt-4o".to_string()),
        HashMap::new(),
    )?;

    let error = client
        .create_message(&[user_message("ping")], None, None, Some(64), None)
        .await
        .err()
        .ok_or_else(|| std::io::Error::other("expected stream error"))?;

    assert!(error.to_string().contains("openai stream failed"));

    Ok(())
}

#[tokio::test]
async fn anthropic_stream_errors_surface_from_create_message() -> TestResult {
    let server = spawn_server(
        "200 OK",
        [
            r#"data: {"type":"error","error":{"message":"anthropic stream failed"}}"#,
            "data: [DONE]",
        ]
        .join("\n"),
    )
    .await?;
    let client = ApiClient::with_provider(
        ProviderKind::Anthropic,
        Some("test-key".to_string()),
        Some(server.base_url),
        Some("claude-sonnet-4-6-20250514".to_string()),
        HashMap::new(),
    )?;

    let error = client
        .create_message(&[user_message("ping")], None, None, Some(64), None)
        .await
        .err()
        .ok_or_else(|| std::io::Error::other("expected stream error"))?;

    assert!(error.to_string().contains("anthropic stream failed"));

    Ok(())
}
