use serde::Deserialize;

/// Content block from JS — typed deserialization for queryWithContent.
#[derive(Deserialize)]
#[serde(tag = "type")]
pub(crate) enum JsContent {
    #[serde(rename = "text")]
    Text { text: String },
    #[serde(rename = "image")]
    Image {
        #[serde(rename = "mimeType")]
        mime_type: String,
        source: JsImageSource,
    },
}

#[derive(Deserialize)]
#[serde(tag = "type")]
pub(crate) enum JsImageSource {
    #[serde(rename = "path")]
    Path { path: String },
    #[serde(rename = "base64")]
    Base64 { data: String },
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
            JsContent::Image { mime_type, source } => match source {
                JsImageSource::Path { path } => {
                    if path.is_empty() {
                        return None;
                    }
                    Some(evot_engine::Content::Image {
                        mime_type,
                        source: evot_engine::ImageSource::Path { path },
                    })
                }
                JsImageSource::Base64 { data } => {
                    if data.is_empty() {
                        return None;
                    }
                    match evot_engine::resize_image(&data, &mime_type) {
                        Ok((resized_data, new_mime)) => Some(evot_engine::Content::Image {
                            mime_type: new_mime,
                            source: evot_engine::ImageSource::Base64 { data: resized_data },
                        }),
                        Err(e) => {
                            eprintln!("image resize failed, using original: {e}");
                            Some(evot_engine::Content::Image {
                                mime_type,
                                source: evot_engine::ImageSource::Base64 { data },
                            })
                        }
                    }
                }
            },
            _ => None,
        })
        .collect();

    Ok(input)
}
