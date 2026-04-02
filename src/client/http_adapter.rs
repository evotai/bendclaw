use crate::types::ErrorCode;
use crate::types::HttpTransportError;

pub(crate) fn to_cluster_dispatch(operation: &str, err: HttpTransportError) -> ErrorCode {
    ErrorCode::cluster_dispatch(format!("{operation}: {err}"))
}

pub(crate) fn to_cluster_collect(operation: &str, err: HttpTransportError) -> ErrorCode {
    ErrorCode::cluster_collect(format!("{operation}: {err}"))
}

pub(crate) fn to_cluster_registration(operation: &str, err: HttpTransportError) -> ErrorCode {
    ErrorCode::cluster_registration(format!("{operation}: {err}"))
}

pub(crate) fn to_cluster_discovery(operation: &str, err: HttpTransportError) -> ErrorCode {
    ErrorCode::cluster_discovery(format!("{operation}: {err}"))
}

pub(crate) fn to_internal(operation: &str, err: HttpTransportError) -> ErrorCode {
    ErrorCode::internal(format!("{operation}: {err}"))
}
