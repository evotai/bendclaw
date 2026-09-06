use axum::response::Html;
use axum::response::IntoResponse;
use axum::response::Redirect;
use axum::routing::get;
use axum::Router;

/// Embedded console assets need no application state. Keep imports and routes
/// together so new frontend modules do not expand the business HTTP server.
pub(super) fn router() -> Router {
    let mut router = Router::new()
        .route(
            "/chat",
            get(|| async { Html(include_str!("static/index.html")) }),
        )
        .route(
            "/models",
            get(|| async { Html(include_str!("static/ui/models.html")) }),
        )
        .route(
            "/feishu",
            get(|| async { Html(include_str!("static/ui/feishu.html")) }),
        )
        .route("/", get(|| async { Redirect::to("/chat") }))
        .route(
            "/settings",
            get(|| async { Redirect::permanent("/models") }),
        )
        .route(
            "/brand/icon.png",
            get(|| async {
                (
                    [
                        ("content-type", "image/png"),
                        ("cache-control", "public, max-age=604800"),
                    ],
                    include_bytes!("static/brand/icon.png").as_slice(),
                )
            }),
        );
    for (path, body) in [
        ("/ui/app.js", include_str!("static/ui/app.js")),
        (
            "/ui/json-client.js",
            include_str!("static/ui/json-client.js"),
        ),
        ("/ui/chat-state.js", include_str!("static/ui/chat-state.js")),
        (
            "/ui/chat-stream-state.js",
            include_str!("static/ui/chat-stream-state.js"),
        ),
        ("/ui/chat.js", include_str!("static/ui/chat.js")),
        (
            "/ui/chat-control.js",
            include_str!("static/ui/chat-control.js"),
        ),
        (
            "/ui/chat-transport.js",
            include_str!("static/ui/chat-transport.js"),
        ),
        ("/ui/chrome.js", include_str!("static/ui/chrome.js")),
        ("/ui/models.js", include_str!("static/ui/models.js")),
        ("/ui/feishu.js", include_str!("static/ui/feishu.js")),
    ] {
        router = router.route(
            path,
            get(move || async move { asset(body, "text/javascript; charset=utf-8") }),
        );
    }
    for (path, body) in [
        ("/ui/app.css", include_str!("static/ui/app.css")),
        ("/ui/chat.css", include_str!("static/ui/chat.css")),
    ] {
        router = router.route(
            path,
            get(move || async move { asset(body, "text/css; charset=utf-8") }),
        );
    }
    router
}

fn asset(body: &'static str, content_type: &'static str) -> impl IntoResponse {
    (
        [
            ("content-type", content_type),
            ("cache-control", "no-cache"),
        ],
        body,
    )
}
