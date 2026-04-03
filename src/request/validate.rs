use super::invocation::InvocationRequest;
use crate::types::ErrorCode;
use crate::types::Result;

pub fn validate(req: &InvocationRequest) -> Result<()> {
    if req.agent_id.is_empty() {
        return Err(ErrorCode::invalid_input("agent_id must not be empty"));
    }
    if req.user_id.is_empty() {
        return Err(ErrorCode::invalid_input("user_id must not be empty"));
    }
    Ok(())
}
