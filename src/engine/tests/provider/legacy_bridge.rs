use std::sync::Arc;
use std::sync::Mutex;

use evotengine::provider::ProviderError;
use evotengine::provider::StreamConfig;
use evotengine::provider::StreamEvent;
use evotengine::provider::StreamOutcome;
use evotengine::provider::StreamProvider;
use evotengine::Content;
use evotengine::Message;
use evotengine::StopReason;
use evotengine::Usage;
use tokio::sync::mpsc;
use tokio_util::sync::CancellationToken;

use crate::fixtures::stream_config::StreamConfigBuilder;

type TestResult = Result<(), Box<dyn std::error::Error>>;
struct Legacy {
    retain: Arc<Mutex<Option<mpsc::UnboundedSender<StreamEvent>>>>,
    wait: bool,
    count: usize,
    started: CancellationToken,
    dropped: CancellationToken,
    fail: bool,
}
#[async_trait::async_trait]
impl StreamProvider for Legacy {
    async fn stream(
        &self,
        _config: StreamConfig,
        tx: mpsc::UnboundedSender<StreamEvent>,
        _cancel: CancellationToken,
    ) -> Result<StreamOutcome, ProviderError> {
        let _dropped = self.dropped.clone().drop_guard();
        *self
            .retain
            .lock()
            .map_err(|_| ProviderError::Other("fixture lock".into()))? = Some(tx.clone());
        for _ in 0..self.count {
            let _ = tx.send(StreamEvent::Start);
        }
        self.started.cancel();
        if self.wait {
            std::future::pending::<()>().await;
        }
        if self.fail {
            return Err(ProviderError::Other("fixture failure".into()));
        }
        Ok(StreamOutcome::complete(Message::Assistant {
            content: vec![Content::Text {
                text: "fixture".into(),
            }],
            stop_reason: StopReason::Stop,
            model: "fixture".into(),
            provider: "fixture".into(),
            usage: Usage::default(),
            timestamp: 0,
            error_message: None,
            response_id: None,
        }))
    }
}
fn fixture(wait: bool, count: usize) -> Legacy {
    Legacy {
        retain: Arc::new(Mutex::new(None)),
        wait,
        count,
        started: CancellationToken::new(),
        dropped: CancellationToken::new(),
        fail: false,
    }
}

#[tokio::test]
async fn completed_provider_does_not_wait_for_retained_sender_and_drains_order() -> TestResult {
    let provider = fixture(false, 3);
    let (tx, mut rx) = mpsc::channel(1);
    let consume = async {
        let mut count = 0;
        while rx.recv().await.is_some() {
            count += 1;
        }
        count
    };
    let work = async {
        tokio::join!(
            provider.stream_bounded(
                StreamConfigBuilder::openai().build(),
                tx,
                CancellationToken::new()
            ),
            consume
        )
    };
    let (result, count) = tokio::time::timeout(std::time::Duration::from_secs(1), work).await?;
    result?;
    assert_eq!(count, 3);
    assert!(provider
        .retain
        .lock()
        .map_err(|_| "lock")?
        .as_ref()
        .is_some_and(|tx| tx.is_closed()));
    Ok(())
}

#[tokio::test]
async fn cancel_or_receiver_drop_releases_uncooperative_provider() -> TestResult {
    for close_receiver in [false, true] {
        let provider = Arc::new(fixture(true, 4));
        let (tx, rx) = mpsc::channel(1);
        let cancel = CancellationToken::new();
        let copied = provider.clone();
        let token = cancel.clone();
        let task = tokio::spawn(async move {
            copied
                .stream_bounded(StreamConfigBuilder::openai().build(), tx, token)
                .await
        });
        tokio::time::timeout(
            std::time::Duration::from_secs(1),
            provider.started.cancelled(),
        )
        .await?;
        if close_receiver {
            drop(rx);
        } else {
            cancel.cancel();
        }
        let result = tokio::time::timeout(std::time::Duration::from_secs(1), task).await??;
        assert!(matches!(result, Err(ProviderError::Cancelled)));
        assert!(provider.dropped.is_cancelled());
    }
    Ok(())
}

#[tokio::test]
async fn failed_provider_drains_accepted_events_before_reporting_error() -> TestResult {
    let mut provider = fixture(false, 3);
    provider.fail = true;
    let (tx, mut rx) = mpsc::channel(1);
    let collect = async {
        let mut count = 0;
        while rx.recv().await.is_some() {
            count += 1;
        }
        count
    };
    let (result, count) = tokio::time::timeout(std::time::Duration::from_secs(1), async {
        tokio::join!(
            provider.stream_bounded(
                StreamConfigBuilder::openai().build(),
                tx,
                CancellationToken::new()
            ),
            collect
        )
    })
    .await?;
    assert!(matches!(result, Err(ProviderError::Other(message)) if message == "fixture failure"));
    assert_eq!(count, 3);
    Ok(())
}

#[tokio::test]
async fn burst_overflow_is_explicit_and_not_retryable() -> TestResult {
    let provider = fixture(true, 300);
    let (tx, _rx) = mpsc::channel(1);
    let work = provider.stream_bounded(
        StreamConfigBuilder::openai().build(),
        tx,
        CancellationToken::new(),
    );
    let result = tokio::time::timeout(std::time::Duration::from_secs(1), work).await?;
    assert!(
        matches!(&result, Err(ProviderError::Other(message)) if message.contains("backlog exceeded"))
    );
    assert!(provider.dropped.is_cancelled());
    Ok(())
}
