use crate::bundle;
use anyhow::{ensure, Context, Result};
use base64::{engine::general_purpose::STANDARD, Engine};
use serde::Serialize;
use serde_json::Value;
use std::path::Path;

#[derive(Debug, Serialize)]
pub struct ImageRef {
    pub label: String,
    #[serde(skip)]
    reference: String,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Message {
    pub role: String,
    pub text: String,
    pub timestamp: Option<String>,
    pub truncated: bool,
    pub images: Vec<ImageRef>,
}

fn image_refs(value: &Value) -> Vec<ImageRef> {
    value
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|part| {
            let reference = if let Some(reference) = part.as_str() {
                reference
            } else {
                if !matches!(
                    part["type"].as_str(),
                    Some("local_image" | "image" | "input_image" | "Image")
                ) {
                    return None;
                }
                part["path"]
                    .as_str()
                    .or_else(|| part["image_url"].as_str())
                    .or_else(|| part["image_url"]["url"].as_str())
                    .or_else(|| part["url"].as_str())?
            };
            let label = if reference.starts_with("data:") {
                "内嵌图片".to_owned()
            } else {
                reference
                    .rsplit(['/', '\\'])
                    .next()
                    .unwrap_or("图片")
                    .to_owned()
            };
            Some(ImageRef {
                label,
                reference: reference.to_owned(),
            })
        })
        .collect()
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Conversation {
    pub title: String,
    pub total: usize,
    pub offset: usize,
    pub messages: Vec<Message>,
    pub warnings: Vec<String>,
}

fn content(value: &Value) -> String {
    if let Some(text) = value.as_str() {
        return text.to_owned();
    }
    if let Some(parts) = value.as_array() {
        return parts
            .iter()
            .filter_map(|part| part["text"].as_str().map(str::to_owned))
            .collect::<Vec<_>>()
            .join("\n");
    }
    String::new()
}

// Each turn can contain several projections of the same message. Prefer completed items.
pub fn messages(records: &[Value]) -> Vec<Message> {
    let mut output = Vec::new();
    let mut channels: [Vec<(usize, Message)>; 3] = Default::default();
    let flush = |channels: &mut [Vec<(usize, Message)>; 3], output: &mut Vec<Message>| {
        let mut selected = Vec::new();
        for role in ["user", "assistant"] {
            if let Some(index) = channels
                .iter()
                .position(|items| items.iter().any(|(_, message)| message.role == role))
            {
                let mut remaining = Vec::new();
                for entry in channels[index].drain(..) {
                    if entry.1.role == role {
                        selected.push(entry);
                    } else {
                        remaining.push(entry);
                    }
                }
                channels[index] = remaining;
            }
        }
        selected.sort_by_key(|(index, _)| *index);
        output.extend(selected.into_iter().map(|(_, message)| message));
        channels.iter_mut().for_each(Vec::clear);
    };
    for (index, row) in records.iter().enumerate() {
        let p = &row["payload"];
        if row["type"] == "event_msg" && p["type"] == "task_started" {
            flush(&mut channels, &mut output);
        }
        let item = &p["item"];
        let candidate = if row["type"] == "event_msg" && p["type"] == "item_completed" {
            match item["type"].as_str() {
                Some("UserMessage") => Some((0, "user", content(&item["content"]))),
                Some("AgentMessage") => Some((
                    0,
                    "assistant",
                    content(item.get("content").unwrap_or(&item["text"])),
                )),
                _ => None,
            }
        } else if row["type"] == "response_item" && p["type"] == "message" {
            match p["role"].as_str() {
                Some("user") => Some((1, "user", content(&p["content"]))),
                Some("assistant") => Some((1, "assistant", content(&p["content"]))),
                _ => None,
            }
        } else if row["type"] == "event_msg" {
            match p["type"].as_str() {
                Some("user_message") => Some((2, "user", content(&p["message"]))),
                Some("agent_message") => Some((2, "assistant", content(&p["message"]))),
                _ => None,
            }
        } else {
            None
        };
        let images = if row["type"] == "event_msg" && p["type"] == "item_completed" {
            image_refs(&item["content"])
        } else if row["type"] == "response_item" {
            image_refs(&p["content"])
        } else {
            image_refs(&p["images"])
        };
        if let Some((channel, role, mut text)) = candidate {
            if !text.is_empty() || !images.is_empty() {
                let boundary = text.char_indices().nth(128 * 1024).map(|(index, _)| index);
                if let Some(index) = boundary {
                    text.truncate(index);
                }
                channels[channel].push((
                    index,
                    Message {
                        role: role.to_owned(),
                        text,
                        timestamp: row["timestamp"].as_str().map(str::to_owned),
                        truncated: boundary.is_some(),
                        images,
                    },
                ));
            }
        }
        if row["type"] == "event_msg" && p["type"] == "task_complete" {
            flush(&mut channels, &mut output);
        }
    }
    flush(&mut channels, &mut output);
    output
}

pub fn image(
    home: Option<&Path>,
    package_path: Option<&Path>,
    id: &str,
    message_index: usize,
    image_index: usize,
) -> Result<String> {
    ensure!(
        home.is_some() != package_path.is_some(),
        "请选择本机会话或导出包"
    );
    let package = package_path.map(bundle::load).transpose()?;
    let records = if let Some(home) = home {
        bundle::local_preview_records(home, id)?.1
    } else {
        let packed = package.as_ref().context("缺少导出包")?;
        let thread = packed
            .manifest
            .threads
            .iter()
            .find(|t| t.id == id)
            .context("会话不存在")?;
        bundle::decode_records(packed.files.get(&thread.file).context("会话文件缺失")?)?
    };
    let messages = messages(&records);
    let reference = &messages
        .get(message_index)
        .context("消息不存在")?
        .images
        .get(image_index)
        .context("图片引用不存在")?
        .reference;
    const LIMIT: usize = 20 * 1024 * 1024;
    let bytes = if reference.starts_with("data:") {
        ensure!(reference.len() <= LIMIT * 4 / 3 + 1024, "图片超过 20 MiB");
        let (header, data) = reference.split_once(',').context("图片数据无效")?;
        ensure!(header.ends_with(";base64"), "不支持此内嵌图片格式");
        STANDARD.decode(data).context("图片数据无效")?
    } else if let Some(packed) = &package {
        let asset = packed
            .manifest
            .assets
            .iter()
            .find(|a| crate::model::path_key(&a.source_path) == crate::model::path_key(reference))
            .context("导出包未包含这张图片")?;
        let bytes = packed.files.get(&asset.file).context("包内图片缺失")?;
        ensure!(bytes.len() <= LIMIT, "图片超过 20 MiB");
        bytes.clone()
    } else {
        ensure!(
            !reference.starts_with("http://") && !reference.starts_with("https://"),
            "远程图片不会自动加载"
        );
        use std::io::Read;
        let mut bytes = Vec::new();
        std::fs::File::open(reference)
            .context("原图不存在或无法读取")?
            .take((LIMIT + 1) as u64)
            .read_to_end(&mut bytes)?;
        bytes
    };
    ensure!(bytes.len() <= LIMIT, "图片超过 20 MiB");
    let mime = if bytes.starts_with(b"\x89PNG\r\n\x1a\n") {
        "image/png"
    } else if bytes.starts_with(b"\xff\xd8\xff") {
        "image/jpeg"
    } else if bytes.starts_with(b"GIF87a") || bytes.starts_with(b"GIF89a") {
        "image/gif"
    } else if bytes.starts_with(b"RIFF") && bytes.get(8..12) == Some(b"WEBP") {
        "image/webp"
    } else {
        anyhow::bail!("不支持的图片格式（支持 PNG、JPEG、GIF、WebP）")
    };
    Ok(format!("data:{mime};base64,{}", STANDARD.encode(bytes)))
}

pub fn preview(
    home: Option<&Path>,
    package: Option<&Path>,
    id: &str,
    offset: usize,
) -> Result<Conversation> {
    ensure!(
        home.is_some() != package.is_some(),
        "请选择本机会话或导出包中的会话"
    );
    let (title, records, warnings) = if let Some(home) = home {
        bundle::local_preview_records(home, id)?
    } else {
        let package = bundle::load(package.context("缺少导出包")?)?;
        let thread = package
            .manifest
            .threads
            .iter()
            .find(|t| t.id == id)
            .context("导出包中未找到该会话")?;
        (
            thread.title.clone(),
            bundle::decode_records(package.files.get(&thread.file).context("会话文件缺失")?)?,
            Vec::new(),
        )
    };
    let all = messages(&records);
    ensure!(offset <= all.len(), "预览页超出消息范围");
    let total = all.len();
    Ok(Conversation {
        title,
        total,
        offset,
        messages: all.into_iter().skip(offset).take(50).collect(),
        warnings,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn supports_both_formats_without_duplicate_projections() {
        let rows = vec![
            json!({"type":"event_msg","payload":{"type":"task_started"}}),
            json!({"type":"event_msg","payload":{"type":"user_message","message":"你好"}}),
            json!({"type":"response_item","payload":{"type":"message","role":"user","content":[{"text":"你好"}]}}),
            json!({"type":"event_msg","payload":{"type":"item_completed","item":{"type":"UserMessage","content":[{"type":"text","text":"你好"},{"type":"local_image","path":"photo.png"}]}}}),
            json!({"type":"event_msg","payload":{"type":"item_completed","item":{"type":"AgentMessage","content":[{"type":"Text","text":"<script>not HTML</script>"}]}}}),
            json!({"type":"event_msg","payload":{"type":"task_complete"}}),
            json!({"type":"response_item","payload":{"type":"message","role":"assistant","content":[{"type":"output_text","text":"旧格式回复"}]}}),
        ];
        let result = messages(&rows);
        assert_eq!(result.len(), 3);
        assert_eq!(result[0].text, "你好");
        assert_eq!(result[0].images[0].label, "photo.png");
        assert_eq!(result[1].role, "assistant");
        assert_eq!(result[2].text, "旧格式回复");
    }

    #[test]
    fn truncation_preserves_unicode_and_is_explicit() {
        let rows = vec![
            json!({"type":"event_msg","payload":{"type":"user_message","message":"中".repeat(128 * 1024 + 1)}}),
        ];
        let result = messages(&rows);
        assert!(result[0].truncated);
        assert_eq!(result[0].text.chars().count(), 128 * 1024);
    }

    #[test]
    fn a_partial_projection_does_not_hide_the_other_role() {
        let rows = vec![
            json!({"type":"event_msg","payload":{"type":"item_completed","item":{"type":"UserMessage","content":[{"text":"Question"}]}}}),
            json!({"type":"response_item","payload":{"type":"message","role":"assistant","content":[{"text":"Reply"}]}}),
        ];
        let result = messages(&rows);
        assert_eq!(result.len(), 2);
        assert_eq!(result[1].text, "Reply");
        let rows = vec![
            json!({"type":"event_msg","payload":{"type":"item_completed","item":{"type":"AgentMessage","text":"Plain reply"}}}),
        ];
        assert_eq!(messages(&rows)[0].text, "Plain reply");
    }

    #[test]
    fn image_only_messages_and_embedded_images_are_retained() {
        let rows = vec![
            serde_json::json!({"type":"response_item","payload":{"type":"message","role":"user","content":[{"type":"input_image","image_url":"data:image/png;base64,dGVzdA=="}]}}),
        ];
        let result = messages(&rows);
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].images.len(), 1);
        assert_eq!(result[0].images[0].label, "内嵌图片");
    }
}
