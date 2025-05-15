```bash
      /\_/\
     ( ^.^ )
    /)  "  (\
   ( | ইহাই | )
   (_| ___ |_)
      U   U         MEOWI 🐱
```
A cozy and powerful terminal companion for chatting with your favorite LLMs.

## What is Meowi?

Meowi is a sleek, terminal-based application (TUI) designed for seamless interaction with a variety of Large Language Models. It brings the power of AI chat to your command line, offering a user-friendly interface to manage conversations, switch between different AI providers and models, and customize your chat experience—all without leaving the comfort of your terminal!

## Features ✨

*   **Multi-Provider Support:** Connect to OpenAI, Anthropic, Grok, OpenRouter, and other OpenAI-compatible APIs.
*   **Custom Endpoints:** Add and use your own self-hosted or custom LLM endpoints.
*   **Syntax Highlighting:** Code blocks in chat messages are beautifully highlighted for readability.
*   **Customizable Prompts:** Define, manage, and toggle system prompts to guide AI behavior for each chat.
*   **Model Selection:** Quickly switch between different models from your configured providers.
*   **Vim-Inspired Keybindings:** Efficient navigation and interaction in vim style (Normal, Insert, Visual, Command modes).
*   **Clipboard Integration:** Copy messages or individual code blocks to your system clipboard
*   **Persistent History & Config:** Your chats and settings are saved locally for future sessions.
*   **Visual Mode:** Select and copy text directly from the chat view.

## Dependencies

*   **Rust:** (Latest stable version recommended) For building the application.
*   **Clipboard Utilities (Optional, for Linux):**
    *   For Wayland: `wl-copy` (from `wl-clipboard` package).
    *   For X11: `xclip` or `xsel`.
    (Meowi uses `arboard` which attempts to use these or other system methods.)

## Installation 

1.  **Install Rust:**
    If you don't have Rust, install it from [rust-lang.org](https://www.rust-lang.org/).

2.  **Install Clipboard Utilities (Optional, for Linux):**
    *   Debian/Ubuntu:
        ```bash
        sudo apt update
        sudo apt install wl-clipboard xclip # For Wayland and X11
        ```
    *   Fedora:
        ```bash
        sudo dnf install wl-clipboard xclip
        ```
    *   Arch Linux:
        ```bash
        sudo pacman -S wl-clipboard xclip
        ```
    (For other distributions, please use your respective package manager.)

3.  **Clone and Build Meowi:**
    ```bash
    git clone https://github.com/0xdilo/meowi.git 
    cd meowi
    cargo build --release
    ```

4.  **Run Meowi:**
    The compiled binary will be located at `target/release/meowi`.
    You can run it directly:
    ```bash
    ./target/release/meowi
    ```
    Or, move it to a directory in your system's `PATH` for easier access (e.g., `~/.local/bin/` or `/usr/local/bin/`):
    ```bash
    sudo mv target/release/meowi /usr/local/bin/ # Example
    meowi # Then run from anywhere
    ```

## Usage & Keybindings ⌨️

Once Meowi is running, you can interact with it using the following keybindings.

### General

*   `s`: Toggle sidebar visibility.
*   `Tab`: Switch focus between sidebar and chat (if sidebar is visible).
*   `o`: Open settings screen.
*   `m`: Open model selection screen.
*   `Esc`:
    *   Exit current input mode (Insert, Command, API Key, etc.) to Normal mode.
    *   Close full message view.
    *   Clear error/info messages.
*   `:` (in Normal mode): Enter Command mode.
    *   `:q` then `Enter`: Quit Meowi.

---

### Normal Mode (Chat View Focused)

*   `i`: Enter **Insert mode** to type and send messages.
*   `v`: Enter **Visual mode** to select text.
*   `j` or `Down Arrow`: Move cursor down / Scroll chat down.
*   `k` or `Up Arrow`: Move cursor up / Scroll chat up.
*   `Ctrl+d`: Page down (scrolls chat view by half a viewport).
*   `Ctrl+u`: Page up (scrolls chat view by half a viewport).
*   `g`: Go to the top of the current chat.
*   `G`: Go to the bottom of the current chat.
*   `e`: Toggle expansion of a truncated message at the cursor.
*   `c`, `C`, `x`, `X`: Copy the 1st, 2nd, 3rd, or 4th code block (respectively) from the message at the cursor. (Configurable)
*   `n`: Create a new chat.
*   `Enter` (when sidebar focused): Switch to the selected chat or open settings if "Settings" is selected.

---

### Normal Mode (Sidebar Focused)

*   `j` or `Down Arrow`: Move selection down.
*   `k` or `Up Arrow`: Move selection up.
*   `g`: Go to the top of the sidebar (first chat).
*   `G`: Go to the bottom of the sidebar (Settings item).
*   `Enter`:
    *   If a chat is selected: Switch to that chat.
    *   If "Settings" is selected: Open the settings screen.
*   `d`: Delete the selected chat (confirmation may be needed or it's immediate).
*   `r`: Rename the selected chat (enters an input mode).

---

### Insert Mode (for typing messages)

*   Type your message.
*   `Enter`: Send the message to the LLM.
*   `Esc`: Exit Insert mode and return to Normal mode (discards current input).
*   `Backspace`: Delete the last character.

---

### Visual Mode (for text selection in chat)

*   `j`, `k`, `Down Arrow`, `Up Arrow`, `Ctrl+d`, `Ctrl+u`: Move cursor and extend selection.
*   `y`: Yank (copy) the selected text to the clipboard.
*   `Esc`: Exit Visual mode and return to Normal mode.

---

### Command Mode

*   Type a command (e.g., `q` to quit).
*   `Enter`: Execute the command.
*   `Esc`: Exit Command mode and return to Normal mode.
*   `Backspace`: Delete the last character.

---

### Settings Screen

*   `h` or `Left Arrow`: Switch to the previous tab (Providers, Shortcuts, Prompts).
*   `l` or `Right Arrow`: Switch to the next tab.
*   `j` or `Down Arrow`: Navigate down the list of items in the current tab.
*   `k` or `Up Arrow`: Navigate up the list of items.
*   `Enter`:
    *   **Providers Tab:** Toggle provider expansion / Toggle model enabled status / Select "Add Custom Model".
    *   **Prompts Tab:** Edit selected prompt / Select "Add New Prompt".
*   `e` (Providers Tab, on a provider): Edit API key for the selected provider.
*   `d`:
    *   **Providers Tab** (on a custom model): Delete the selected custom model.
    *   **Prompts Tab** (on a prompt): Delete the selected prompt.
*   `Space` (Prompts Tab, on a prompt): Toggle the active status of the selected prompt.
*   `Esc`: Exit settings and return to Normal mode.

