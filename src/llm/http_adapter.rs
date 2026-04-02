use crate::types::ErrorCode;
use crate::types::HttpTransportError;

pub(crate) fn to_llm_error(err: HttpTransportError) -> ErrorCode {
    ErrorCode::llm_request(err.summary())
}

pub(crate) fn to_stream_error(err: HttpTransportError) -> String {
    err.summary()
}
