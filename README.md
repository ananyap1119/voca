# voca

> Tray-app dictation for 10 Indic languages. Codemix Hinglish included. Apple/Google dictation, but actually good.

**Status:** v0.1 — ready to use.

**Sovereignty:** sovereign-by-construction. BYO endpoint, BYO key. A local-only configuration is documented.

This is a community project, **not affiliated with Saaras or Sarvam AI**.
Best-effort community shovel — no SLA, no roadmap commitments.

---

## Architecture

```
┌─────────────────┐     ┌──────────────┐     ┌─────────────────┐
│  Global hotkey  │────▶│   saaras-    │────▶│   Saaras v3     │
│  (Cmd+Shift+S) │     │   tray       │     │   (STT API)     │
└─────────────────┘     │  (Tauri +    │     │   OR local      │
                        │   cpal)      │     │   Whisper       │
                        └──────────────┘     └─────────────────┘
                               │
                               ▼
                        ┌──────────────┐
                        │  Clipboard   │
                        │  + auto-paste│
                        └──────────────┘
```

## What this is

Press a global hotkey, speak in Hindi, Tamil, Telugu, Bengali, Marathi, Gujarati, Kannada, Malayalam, Punjabi, or Urdu — and get perfectly transcribed text pasted into whatever app you're using. Supports Hinglish/Tanglish codemix (speak Hindi, get Latin-script output).

Works on macOS, Windows, and Linux.

## What this isn't

- Not a full keyboard replacement
- Not a translation tool (see [sarvam-translate](https://github.com/sovereign-shovels/sarvam-pdf))
- No offline mode in v0.1 (local Whisper fallback comes in v0.5)

See [PRD-v1.md](./PRD-v1.md) for the full anti-scope definition.

---

## Install

### Pre-built binaries

Coming soon. For now, build from source.

### Build from source

**Prerequisites:**
- [Node.js](https://nodejs.org/) 20+
- [Rust](https://rustup.rs/) 1.75+

```bash
git clone https://github.com/sovereign-shovels/voca.git
cd voca

# Install frontend dependencies
npm install

# Build desktop app without packaging an installer
npm run build

# Or run in dev mode
npm run dev

# Package an installer when you need one
npm run bundle
```

The release binary will be in `src-tauri/target/release/voca.exe`.
Bundled installers, when built, will be in `src-tauri/target/release/bundle/`.

---

## Configure

### Option A: Saaras v3 (cloud, recommended)

Get a free API key from [Sarvam AI Dashboard](https://dashboard.sarvam.ai/). Then:

```bash
export SAARAS_API_KEY="your-key-here"
```

Or set it in your config file:

```toml
# ~/.config/voca/config.toml
[provider]
endpoint = "https://api.sarvam.ai/speech-to-text"
api_key_env_var = "SAARAS_API_KEY"
language = "hi-IN"
codemix = true
```

**Supported languages:** `hi-IN`, `ta-IN`, `te-IN`, `bn-IN`, `mr-IN`, `gu-IN`, `kn-IN`, `ml-IN`, `pa-IN`, `ur-IN`, `en-IN`

**Codemix:** When enabled, the model handles mid-sentence language switching (e.g., Hindi + English).

### Option B: Local-only (no cloud, no key)

Set the provider to `local` for a placeholder fallback. Full local Whisper-Indic integration ships in v0.5.

```toml
# ~/.config/voca/config.toml
[provider]
provider_name = "local"
```

### Environment variables

All config options can be set via env vars (prefix: `SAARAS_TRAY_`):

```bash
export SAARAS_TRAY_LANGUAGE="ta-IN"
export SAARAS_TRAY_CODEMIX="true"
export SAARAS_TRAY_HOTKEY="CmdOrCtrl+Shift+S"
```

### Changing the hotkey

Default: `CmdOrCtrl+Shift+S` (macOS: `Cmd+Shift+S`, Windows/Linux: `Ctrl+Shift+S`)

```toml
# ~/.config/voca/config.toml
[provider]
hotkey = "CmdOrCtrl+Shift+D"
```

---

## Usage

1. Launch the app. It lives in your system tray.
2. Press the hotkey (`Cmd/Ctrl+Shift+S` by default).
3. Speak for up to 5 seconds.
4. The transcribed text is automatically pasted into your active app.

Click the tray icon to open Settings and change language, provider, or hotkey.

**Verified:** `cargo test` passes (2 tests: Saaras provider + local Whisper placeholder).

---

## Why this exists

Indic dictation on macOS, Windows, and Linux is genuinely broken. Apple's dictation for Hindi/Tamil/Telugu has been bad for years. Google's only works in Chrome. Indians who think and write in their first language type slower than they think — and that's a quality-of-life problem at scale.

See [PRD-v1.md](./PRD-v1.md) for the full problem statement and rationale.

## What's next

- **v0.5:** Continuous dictation mode, local Whisper-Indic fallback, custom vocabulary
- **v1.0:** Meeting capture, multi-speaker diarization, voice command shortcuts

See [PRD-v1.md](./PRD-v1.md) for the full roadmap.

---

## License

Apache 2.0. See [LICENSE](./LICENSE).

## Part of sovereign-shovels

This repo is part of the [sovereign-shovels](https://github.com/sovereign-shovels) portfolio of small, focused, sovereign-by-construction AI utilities.

Other shovels: claude-vault, bulbul-studio, voca, claude-prompts, ollama-cron, mcp-forge, sarvam-pdf, agent-console, sarvam-meet, obsidian-llm, llm-diff, claude-bridge, claude-radio, sarvam-cast.
