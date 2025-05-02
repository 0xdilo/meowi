use anyhow::{Context, Result};
use arboard::Clipboard;
use std::env;
use std::ffi::OsStr;
use tokio::io::AsyncWriteExt;
use tokio::process::Command as TokioCommand;

pub async fn copy_to_clipboard(text: &str) -> Result<()> {
    if !is_wayland_session() {
        Clipboard::new()
            .context("Failed to initialize clipboard")?
            .set_text(text)
            .context("Failed to set clipboard text")?;
        return Ok(());
    }

    let mut cmd = TokioCommand::new("wl-copy")
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn()
        .context("Failed to spawn wl-copy process")?;

    if let Some(mut stdin) = cmd.stdin.take() {
        stdin
            .write_all(text.as_bytes())
            .await
            .context("Failed to write to wl-copy stdin")?;
        stdin
            .shutdown()
            .await
            .context("Failed to close wl-copy stdin")?;
    } else {
        return Err(anyhow::anyhow!("Failed to open wl-copy stdin"));
    }

    let status = cmd.wait().await.context("Failed to wait for wl-copy")?;
    if !status.success() {
        return Err(anyhow::anyhow!("wl-copy exited with status: {}", status));
    }
    Ok(())
}

#[inline(always)]
fn is_wayland_session() -> bool {
    env::var_os("WAYLAND_DISPLAY").is_some()
        || env::var_os("XDG_SESSION_TYPE")
            .as_deref()
            .map_or(false, |v| v == OsStr::new("wayland"))
}
