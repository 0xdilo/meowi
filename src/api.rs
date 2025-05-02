use crate::app::Message;
use anyhow::{Result, anyhow};
use futures_util::StreamExt;
use serde_json::json;
use tokio::sync::mpsc::Sender;

pub async fn stream_message(
    api_key: &str,
    provider: &str,
    model: &str,
    messages: &[Message],
    tx: Sender<String>,
) -> Result<()> {
    match provider {
        "Anthropic" => stream_anthropic(api_key, model, messages, tx).await,
        "OpenAI" => {
            stream_openai_compatible(
                "https://api.openai.com/v1/chat/completions",
                Some(api_key),
                model,
                messages,
                tx,
            )
            .await
        }
        "Grok" => {
            stream_openai_compatible(
                "https://api.x.ai/v1/chat/completions",
                Some(api_key),
                model,
                messages,
                tx,
            )
            .await
        }
        "OpenRouter" => {
            stream_openai_compatible(
                "https://openrouter.ai/api/v1/chat/completions",
                Some(api_key),
                model,
                messages,
                tx,
            )
            .await
        }
        _ => Err(anyhow!("Unsupported provider: {}", provider)),
    }
}

pub async fn stream_openai_compatible(
    endpoint: &str,
    api_key: Option<&str>,
    model: &str,
    messages: &[Message],
    tx: Sender<String>,
) -> Result<()> {
    let client = reqwest::Client::new();
    let mut req = client.post(endpoint).json(&json!({
        "model": model,
        "messages": messages,
        "stream": true
    }));
    if let Some(key) = api_key {
        req = req.bearer_auth(key);
    }
    let response = req.send().await?;
    let mut stream = response.bytes_stream();

    while let Some(chunk) = stream.next().await {
        let chunk = chunk?;
        let chunk_str = std::str::from_utf8(&chunk)?;
        for line in chunk_str.lines() {
            if let Some(data) = line.strip_prefix("data: ") {
                if data == "[DONE]" {
                    return Ok(());
                }
                if let Ok(json) = serde_json::from_str::<serde_json::Value>(data) {
                    if let Some(delta) = json
                        .get("choices")
                        .and_then(|c| c.get(0))
                        .and_then(|c| c.get("delta"))
                        .and_then(|d| d.get("content"))
                        .and_then(|c| c.as_str())
                    {
                        let _ = tx.send(delta.to_string()).await;
                    } else if let Some(typ) = json.get("type").and_then(|t| t.as_str()) {
                        if typ == "response.output_text.delta" {
                            if let Some(delta) = json.get("delta").and_then(|d| d.as_str()) {
                                let _ = tx.send(delta.to_string()).await;
                            }
                        }
                    }
                }
            }
        }
    }
    Ok(())
}

pub async fn stream_anthropic(
    api_key: &str,
    model: &str,
    messages: &[Message],
    tx: Sender<String>,
) -> Result<()> {
    let client = reqwest::Client::new();
    let mut stream = client
        .post("https://api.anthropic.com/v1/messages")
        .bearer_auth(api_key)
        .header("x-api-key", api_key)
        .header("anthropic-version", "2023-06-01")
        .json(&json!({
            "model": model,
            "max_tokens": 4096,
            "messages": messages,
            "stream": true
        }))
        .send()
        .await?
        .bytes_stream();

    while let Some(chunk) = stream.next().await {
        let chunk = chunk?;
        let chunk_str = std::str::from_utf8(&chunk)?;
        for line in chunk_str.lines() {
            if let Some(data) = line.strip_prefix("data: ") {
                if data.is_empty() {
                    continue;
                }
                if let Ok(json) = serde_json::from_str::<serde_json::Value>(data) {
                    if let Some(content) = json
                        .get("delta")
                        .and_then(|d| d.get("text"))
                        .and_then(|t| t.as_str())
                    {
                        let _ = tx.send(content.to_string()).await;
                    }
                }
            }
        }
    }
    Ok(())
}
