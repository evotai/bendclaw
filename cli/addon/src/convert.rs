use serde::Deserialize;

/// Content block from JS — typed deserialization for queryWithContent.
#[derive(Deserialize)]
#[serde(tag = "type")]
pub(crate) enum JsContent {
    #[serde(rename = "text")]
    Text { text: String },
    #[serde(rename = "image")]
    Image {
        /// Base64 data — may be empty when `source` is provided.
        #[serde(default)]
        data: String,
        #[serde(rename = "mimeType")]
        mime_type: String,
        /// File path to the cached image on disk.
        #[serde(default)]
        source: Option<String>,
    },
}

/// Convert a JSON string of content blocks into engine Content items.
/// Images are resized to max 2000×2000 before entering context.
pub(crate) fn parse_content_blocks(
    json: &str,
) -> std::result::Result<Vec<evot_engine::Content>, String> {
    let blocks: Vec<JsContent> =
        serde_json::from_str(json).map_err(|e| format!("parse content: {e}"))?;

    let input: Vec<evot_engine::Content> = blocks
        .into_iter()
        .filter_map(|block| match block {
            JsContent::Text { text } if !text.is_empty() => {
                Some(evot_engine::Content::Text { text })
            }
            JsContent::Image {
                data,
                mime_type,
                source,
            } => {
                // When a source path is provided, store it as a lazy reference
                // so the base64 payload stays out of the message context.
                if let Some(ref path) = source {
                    if !path.is_empty() {
                        return Some(evot_engine::Content::Image {
                            data: String::new(),
                            mime_type,
                            source,
                        });
                    }
                }
                // Fallback: inline base64 (e.g. feishu, or no source)
                if data.is_empty() {
                    return None;
                }
                match evot_engine::resize_image(&data, &mime_type) {
                    Ok((resized_data, new_mime)) => Some(evot_engine::Content::Image {
                        data: resized_data,
                        mime_type: new_mime,
                        source: None,
                    }),
                    Err(e) => {
                        eprintln!("image resize failed, using original: {e}");
                        Some(evot_engine::Content::Image {
                            data,
                            mime_type,
                            source: None,
                        })
                    }
                }
            }
            _ => None,
        })
        .collect();

    Ok(input)
}
