# voca

> Tray-app dictation for 10 Indic languages. Codemix Hinglish included. Apple/Google dictation, but actually good.

**Demo:** https://x.com/AnanNo_11/status/2072590289823494479?s=20

**Status:** v0.2 — Windows BYO-key release.

**Provider control:** bring your own Sarvam endpoint and API key. Local inference
is planned but is not implemented in this release.

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

Download the latest Windows installer from the
[Voca releases page](https://github.com/ananyap1119/voca/releases/latest).

The current beta is unsigned, so Windows may show an Unknown Publisher warning.
Download releases only from the repository above.

### Build from source

**Prerequisites:**
- [Node.js](https://nodejs.org/) 20+
- [Rust](https://rustup.rs/) 1.75+

```bash
git clone https://github.com/ananyap1119/voca.git
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

### Saaras v3

Voca uses a bring-your-own-key model. Get an API key from the
[Sarvam AI Dashboard](https://dashboard.sarvam.ai/), open Voca, paste the key
under **Provider**, and select **Save key**. The key is stored in Windows
Credential Manager and is never included in the installer.

Developers can alternatively provide the key through the environment:

```bash
export SAARAS_API_KEY="your-key-here"
```

Advanced provider settings remain available in the config file:

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

### Environment variables

All config options can be set via env vars (prefix: `VOCA_`):

```bash
export VOCA_LANGUAGE="ta-IN"
export VOCA_CODEMIX="true"
export VOCA_HOTKEY="CmdOrCtrl+Shift+S"
```

When upgrading an existing installation, replace the previous app-specific
environment-variable prefix with `VOCA_`. Provider credentials such as
`SAARAS_API_KEY` keep their provider-specific names.

### Changing the hotkey

Default on Windows: hold `Alt` while speaking and release it to transcribe.

```toml
# ~/.config/voca/config.toml
[provider]
hotkey = "Alt"
```

---

## Usage

1. Install and launch Voca.
2. Paste your Sarvam API key in Voca and select **Save key**.
3. Focus any text field, then hold `Alt` while speaking.
4. Release `Alt`. Voca transcribes, polishes, and pastes the text into the
   previously focused application.

Long dictation is split into API-safe parts automatically. Temporary audio files
are removed after transcription.

Click the tray icon to open Settings and change the language or codemix mode.

**Verified:** the Rust test suite covers configuration, audio chunking,
transcription modes, transcript cleanup, and settings preservation.

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
