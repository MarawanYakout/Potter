# 🧙 Potter

> Summon AI instantly, anywhere on your desktop — no terminal, no browser, no friction.

Potter is a lightweight, always-running daemon written in **Rust** that listens for a global hotkey (`Alt+Space` on Linux, `Option+Space` on macOS). Press it and a sleek, frameless overlay window appears in the **top-right corner** of your screen. Type your prompt, get a streaming response from your chosen LLM, and press `Escape` to dismiss.

Think **Alfred / Raycast**, but for LLMs — and fully open-source.

---

## ✨ Features

- **Global hotkey** — `Alt+Space` (Linux) / `Option+Space` (macOS)
- **Frameless overlay** pinned to top-right, always-on-top
- **Streaming responses** — token-by-token, no waiting
- **Multiple LLM backends:**
  - 🌐 Google Gemini (REST API)
  - 🤖 Anthropic Claude (CLI subprocess)
  - 🏠 Local AI — Ollama, LM Studio, llama.cpp (OpenAI-compatible)
- **`@prefix` routing** — `@gemini`, `@claude`, `@local`, `@local:mistral`
- **Prompt history** — `↑/↓` to cycle through past prompts
- **Markdown rendering** — code blocks, bold, lists in the output
- **Copy button** — one-click copy of any response
- **System tray icon** — right-click to configure or quit
- **Python plugin system** — drop `.py` files in `~/.config/potter/plugins/`
- **Auto model discovery** — detects all locally pulled Ollama models
- **Clipboard context** — `Ctrl+V` on open prefixes your clipboard as context

---

## 🚀 Quick Start

### Prerequisites

- Rust 1.78+ (`rustup`)
- GTK4 development libraries
  - Ubuntu/Debian: `sudo apt install libgtk-4-dev`
  - Fedora: `sudo dnf install gtk4-devel`
  - Arch: `sudo pacman -S gtk4`
- For Gemini: a valid `GEMINI_API_KEY` or set in config
- For Claude: [`claude` CLI](https://docs.anthropic.com/en/docs/claude-code) installed and authenticated
- For local AI: [Ollama](https://ollama.com) running (`ollama serve`)

### Build & Run

```bash
git clone https://github.com/MarawanYakout/Potter
cd Potter
cargo build --release
./target/release/potter
```

Potter starts as a background daemon and registers the global hotkey. Press `Alt+Space` to open the overlay.

---

## ⚙️ Configuration

Config file lives at `~/.config/potter/config.toml` (auto-created on first run):

```toml
[defaults]
model = "gemini"          # gemini | claude | local
hotkey = "alt+space"      # alt+space | option+space
window_position = "top-right"  # top-right | bottom-right | center
max_history = 100

[gemini]
api_key = "YOUR_GEMINI_API_KEY"
model = "gemini-2.0-flash"

[claude]
# Uses the `claude` CLI — no API key needed here
# Ensure `claude` is in your PATH and authenticated

[local]
base_url = "http://localhost:11434"  # Ollama default
model = "llama3.2"                   # any model from `ollama list`
# LM Studio:  base_url = "http://localhost:1234/v1"
# llama.cpp:  base_url = "http://localhost:8080"
```

---

## 🔀 LLM Routing

Prefix your prompt to choose the backend:

| Prefix | Routes to |
|---|---|
| *(none)* | Default from `config.toml` |
| `@gemini` | Google Gemini API |
| `@claude` | Claude CLI |
| `@local` | Local model (Ollama/LM Studio/llama.cpp) |
| `@local:mistral` | Local, force specific model name |

---

## ⌨️ Keyboard Shortcuts

| Key | Action |
|---|---|
| `Alt+Space` | Open overlay |
| `Enter` | Submit prompt |
| `Shift+Enter` | New line in prompt |
| `↑ / ↓` | Cycle prompt history |
| `Escape` | Close overlay (while focused) |
| `Ctrl+V` (on open) | Paste clipboard as context prefix |

---

## 🔌 Plugins (Python)

Drop a `.py` file in `~/.config/potter/plugins/`. Each plugin registers a `/command`:

```python
# ~/.config/potter/plugins/translate.py
COMMAND = "/translate"
DESCRIPTION = "Translate text to a target language"

def run(args: str) -> str:
    # args = everything after /translate
    # return a modified prompt string
    return f"Translate the following text to English:\n\n{args}"
```

Then type `/translate Bonjour le monde` in Potter.

---

## 🗺️ Roadmap

- [x] Phase 1: Hotkey → overlay → Gemini streaming → Escape to close
- [ ] Phase 2: Claude CLI + Ollama local backend + `@prefix` routing + config
- [ ] Phase 3: Markdown rendering, prompt history, copy button, system tray
- [ ] Phase 4: Auto model discovery, clipboard context, Python plugins
- [ ] Phase 5: Packaging — `.deb`, `.rpm`, AUR, Homebrew

---

## 🏗️ Architecture

```
Potter Daemon (Rust)
├── hotkey.rs        — rdev global key listener
├── window.rs        — GTK4 frameless overlay window
├── config.rs        — TOML config loader/writer
├── history.rs       — Prompt history ring buffer
└── llm/
    ├── mod.rs       — LlmProvider trait + @prefix router
    ├── gemini.rs    — Google Gemini REST + SSE streaming
    ├── claude.rs    — Claude CLI subprocess wrapper
    └── local.rs     — OpenAI-compat handler (Ollama/LMStudio/llama.cpp)
```

---

## 📄 License

MIT — see [LICENSE](LICENSE)
