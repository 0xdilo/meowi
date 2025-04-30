use anyhow::Result;
use arboard::Clipboard;
use std::env;
use tokio::io::AsyncWriteExt;
use tokio::process::Command as TokioCommand;

pub async fn copy_to_clipboard(text: &str) -> Result<()> {
    let text = text.to_string();
    let is_wayland = env::var("WAYLAND_DISPLAY").is_ok()
        || env::var("XDG_SESSION_TYPE").map_or(false, |v| v == "wayland");

    if is_wayland {
        let mut cmd = TokioCommand::new("wl-copy")
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .spawn()?;

        if let Some(mut stdin) = cmd.stdin.take() {
            stdin.write_all(text.as_bytes()).await?;
            stdin.shutdown().await?; // Close stdin to signal EOF
        }

        let status = cmd.wait().await?;
        if !status.success() {
            return Err(anyhow::anyhow!("Failed to copy to clipboard using wl-copy"));
        }
        Ok(())
    } else {
        let mut clipboard = Clipboard::new()?;
        clipboard.set_text(text)?;
        Ok(())
    }
}
