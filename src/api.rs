use crate::app::Message;
use anyhow::Result;
use futures_util::StreamExt;
use serde_json::json;
use tokio::sync::mpsc::{self, Sender};

pub async fn stream_message(
    api_key: &str,
    provider: &str,
    model: &str,
    messages: &[Message],
    tx: Sender<String>,
) -> Result<()> {
    match provider {
        "OpenAI" => stream_openai(api_key, model, messages, tx).await,
        "Grok" => stream_grok(api_key, model, messages, tx).await,
        _ => Err(anyhow::anyhow!("Unsupported provider")),
    }
}

pub async fn stream_custom_model(
    endpoint: &str,
    api_key: Option<&str>,
    model: &str,
    messages: &[Message],
    tx: Sender<String>,
) -> Result<()> {
    let client = reqwest::Client::new();
    let mut req = client.post(endpoint).json(&serde_json::json!({
        "model": model,
        "input": messages,
        "stream": true
    }));
    println!("{:?}", api_key);
    if let Some(key) = api_key {
        req = req.bearer_auth(key);
    }
    let response = req.send().await;

    // Log response or error to file
    let mut file = tokio::fs::OpenOptions::new()
        .write(true)
        .append(true)
        .create(true)
        .open("/home/polizia/Git/meowi/error.txt")
        .await?;

    match &response {
        Ok(resp) => {
            tokio::io::AsyncWriteExt::write_all(
                &mut file,
                format!("Response status: {}\n", resp.status()).as_bytes(),
            )
            .await?;
        }
        Err(e) => {
            tokio::io::AsyncWriteExt::write_all(&mut file, format!("Error: {}\n", e).as_bytes())
                .await?;
        }
    }
    tokio::io::AsyncWriteExt::flush(&mut file).await?;

    let mut stream = response?.bytes_stream();

    use futures_util::StreamExt;
    while let Some(chunk) = stream.next().await {
        let chunk = chunk?;
        let chunk_str = std::str::from_utf8(&chunk)?;

        // Log chunk to file
        let mut file = tokio::fs::OpenOptions::new()
            .write(true)
            .append(true)
            .create(true)
            .open("/home/polizia/Git/meowi/error.txt")
            .await?;
        tokio::io::AsyncWriteExt::write_all(
            &mut file,
            format!("Chunk: {}\n", chunk_str).as_bytes(),
        )
        .await?;
        tokio::io::AsyncWriteExt::flush(&mut file).await?;

        for line in chunk_str.lines() {
            if line.starts_with("data: ") {
                let data = &line[6..];
                if data == "[DONE]" {
                    break;
                }
                if let Ok(json) = serde_json::from_str::<serde_json::Value>(data) {
                    // Handle OpenAI-style
                    if let Some(delta) = json
                        .get("choices")
                        .and_then(|c| c.get(0))
                        .and_then(|c| c.get("delta"))
                        .and_then(|d| d.get("content"))
                        .and_then(|c| c.as_str())
                    {
                        tx.send(delta.to_string()).await?;
                    }
                    // Handle custom model style
                    else if let Some(typ) = json.get("type").and_then(|t| t.as_str()) {
                        if typ == "response.output_text.delta" {
                            if let Some(delta) = json.get("delta").and_then(|d| d.as_str()) {
                                tx.send(delta.to_string()).await?;
                            }
                        }
                    }
                }
            }
        }
    }
    Ok(())
}

async fn stream_openai(
    api_key: &str,
    model: &str,
    messages: &[Message],
    tx: Sender<String>,
) -> Result<()> {
    let client = reqwest::Client::new();
    let mut stream = client
        .post("https://api.openai.com/v1/chat/completions")
        .bearer_auth(api_key)
        .json(&json!({
            "model": model,
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
            if line.starts_with("data: ") {
                let data = &line[6..];
                if data == "[DONE]" {
                    break;
                }
                if let Ok(json) = serde_json::from_str::<serde_json::Value>(data) {
                    if let Some(delta) = json["choices"][0]["delta"]["content"].as_str() {
                        // Append only the text content to file
                        let mut file = tokio::fs::OpenOptions::new()
                            .write(true)
                            .append(true)
                            .create(true)
                            .open("/home/polizia/Git/meowi/temp.txt")
                            .await?;
                        tokio::io::AsyncWriteExt::write_all(&mut file, delta.as_bytes()).await?;
                        tokio::io::AsyncWriteExt::flush(&mut file).await?;
                        tx.send(delta.to_string()).await?;
                    }
                }
            }
        }
    }
    Ok(())
}

async fn stream_grok(
    api_key: &str,
    model: &str,
    messages: &[Message],
    tx: Sender<String>,
) -> Result<()> {
    let client = reqwest::Client::new();
    let mut stream = client
        .post("https://api.x.ai/v1/chat/completions") // Adjust to xAI's endpoint if different
        .bearer_auth(api_key)
        .json(&json!({
            "model": model,
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
            if line.starts_with("data: ") {
                let data = &line[6..];
                if data == "[DONE]" {
                    break;
                }
                if let Ok(json) = serde_json::from_str::<serde_json::Value>(data) {
                    if let Some(delta) = json["choices"][0]["delta"]["content"].as_str() {
                        // Append only the text content to file
                        let mut file = tokio::fs::OpenOptions::new()
                            .write(true)
                            .append(true)
                            .create(true)
                            .open("/home/polizia/Git/meowi/temp.txt")
                            .await?;
                        tokio::io::AsyncWriteExt::write_all(&mut file, delta.as_bytes()).await?;
                        tokio::io::AsyncWriteExt::flush(&mut file).await?;
                        tx.send(delta.to_string()).await?;
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
        .json(&json!({
            "model": model,
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
            if line.starts_with("data: ") {
                let data = &line[6..];
                if data == "[DONE]" {
                    break;
                }
                if let Ok(json) = serde_json::from_str::<serde_json::Value>(data) {
                    if let Some(delta) = json["choices"][0]["delta"]["content"].as_str() {
                        let mut file = tokio::fs::OpenOptions::new()
                            .write(true)
                            .append(true)
                            .create(true)
                            .open("/home/polizia/Git/meowi/temp.txt")
                            .await?;
                        tokio::io::AsyncWriteExt::write_all(&mut file, delta.as_bytes()).await?;
                        tokio::io::AsyncWriteExt::flush(&mut file).await?;
                        tx.send(delta.to_string()).await?;
                    }
                }
            }
        }
    }
    Ok(())
}
