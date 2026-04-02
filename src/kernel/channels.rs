/// Re-export bridge: `channels` -> `channel`.
///
/// The plan renames `channel/` to `channels/`. During transition, new code can
/// import from `kernel::channels::` and it resolves to the same types.
pub use super::channel::*;
