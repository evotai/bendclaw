//! Compatibility for providers implementing only the original channel API.
//! No detached task is created. Completion closes retained sender clones;
//! cancellation drops the provider future even if it ignores its token.
//!
//! This is a cooperative admission guard, not a hard memory bound: a legacy
//! provider can enqueue arbitrarily much in one poll. Implement stream_bounded
//! for backpressure at the producer, as the built-in HTTP providers do.
use std::time::Duration;

use tokio::sync::mpsc;
use tokio_util::sync::CancellationToken;

use super::error::ProviderError;
use super::traits::StreamConfig;
use super::traits::StreamEvent;
use super::traits::StreamOutcome;
use super::traits::StreamProvider;

const MAX_LEGACY_PENDING: usize = 256;

pub(super) async fn stream<P: StreamProvider + ?Sized>(
    provider: &P,
    config: StreamConfig,
    tx: mpsc::Sender<StreamEvent>,
    cancel: CancellationToken,
) -> Result<StreamOutcome, ProviderError> {
    let local = cancel.child_token();
    let _cancel_on_exit = local.clone().drop_guard();
    let (legacy_tx, mut rx) = mpsc::unbounded_channel();
    let produce = provider.stream(config, legacy_tx, local);
    tokio::pin!(produce);
    let mut outcome = None;
    let mut pending = None;
    let mut guard_tick = tokio::time::interval(Duration::from_millis(25));
    loop {
        if rx.len() > MAX_LEGACY_PENDING {
            return Err(ProviderError::Other(
                "Legacy provider event backlog exceeded 256; implement stream_bounded to support backpressure".into(),
            ));
        }
        if outcome.is_some() && pending.is_none() && rx.is_empty() {
            return outcome
                .ok_or_else(|| ProviderError::Other("Missing provider outcome".into()))?;
        }
        tokio::select! {
            biased;
            _ = cancel.cancelled() => {
                // Let cooperative implementations observe cancellation once;
                // never wait for an implementation that ignores its token.
                if outcome.is_none() { let _ = futures::poll!(&mut produce); }
                return Err(ProviderError::Cancelled);
            }
            _ = tx.closed() => return Err(ProviderError::Cancelled),
            result = &mut produce, if outcome.is_none() => {
                outcome = Some(result);
                // Do not wait for third-party clones to be dropped after return.
                rx.close();
            }
            permit = tx.reserve(), if pending.is_some() => {
                let permit = permit.map_err(|_| ProviderError::Cancelled)?;
                if let Some(event) = pending.take() { permit.send(event); }
            }
            event = rx.recv(), if pending.is_none() && (!rx.is_closed() || !rx.is_empty()) => {
                pending = event;
            }
            _ = guard_tick.tick() => {}
        }
    }
}
