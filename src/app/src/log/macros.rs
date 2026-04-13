#[macro_export]
macro_rules! logx {
    ($level:ident, $stage:expr, $status:expr, msg = $msg:expr, $($rest:tt)*) => {
        tracing::$level!(
            stage = $stage,
            status = $status,
            $($rest)*
            "{}",
            $msg
        )
    };
    ($level:ident, $stage:expr, $status:expr, msg = $msg:expr $(,)?) => {
        tracing::$level!(
            stage = $stage,
            status = $status,
            "{}",
            $msg
        )
    };
    ($level:ident, $stage:expr, $status:expr, $($rest:tt)*) => {
        tracing::$level!(
            stage = $stage,
            status = $status,
            $($rest)*
            concat!($stage, " ", $status)
        )
    };
    ($level:ident, $stage:expr, $status:expr $(,)?) => {
        tracing::$level!(
            stage = $stage,
            status = $status,
            concat!($stage, " ", $status)
        )
    };
}
