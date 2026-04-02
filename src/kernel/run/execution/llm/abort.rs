//! Abort policy for the query engine run loop.

use std::time::Instant;

use crate::kernel::run::result::Reason;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AbortSignal {
    None,
    Aborted,
    Timeout,
    MaxIterations,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LoopDecision {
    pub signal: AbortSignal,
    pub reason: Option<Reason>,
}

#[derive(Debug, Clone, Copy)]
pub struct AbortPolicy {
    max_iterations: u32,
}

impl AbortPolicy {
    pub fn new(max_iterations: u32) -> Self {
        Self { max_iterations }
    }

    pub fn check(
        &self,
        cancelled: bool,
        now: Instant,
        deadline: Instant,
        iterations: u32,
    ) -> LoopDecision {
        let signal = if cancelled {
            AbortSignal::Aborted
        } else if now >= deadline {
            AbortSignal::Timeout
        } else if iterations >= self.max_iterations {
            AbortSignal::MaxIterations
        } else {
            AbortSignal::None
        };
        LoopDecision {
            reason: match signal {
                AbortSignal::None => None,
                AbortSignal::Aborted => Some(Reason::Aborted),
                AbortSignal::Timeout => Some(Reason::Timeout),
                AbortSignal::MaxIterations => Some(Reason::MaxIterations),
            },
            signal,
        }
    }

    pub fn check_cancel_or_timeout(
        &self,
        cancelled: bool,
        now: Instant,
        deadline: Instant,
    ) -> LoopDecision {
        let signal = if cancelled {
            AbortSignal::Aborted
        } else if now >= deadline {
            AbortSignal::Timeout
        } else {
            AbortSignal::None
        };
        LoopDecision {
            reason: match signal {
                AbortSignal::Aborted => Some(Reason::Aborted),
                AbortSignal::Timeout => Some(Reason::Timeout),
                _ => None,
            },
            signal,
        }
    }
}
