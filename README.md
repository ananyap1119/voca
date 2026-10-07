# voca

> Windows push-to-talk dictation prototype powered by Sarvam Saaras.

**Demo:** https://x.com/AnanNo_11/status/2072590289823494479?s=20

**Status:** v0.2 — Windows BYO-key release.

**Provider control:** bring your own Sarvam endpoint and API key. This release
uses Sarvam's batch speech-to-text API; local inference is not implemented.

This is a community project, **not affiliated with Saaras or Sarvam AI**.
Best-effort community shovel — no SLA, no roadmap commitments.

---

## Architecture

```text
Right Alt press/release -> CPAL microphone capture -> temporary PCM WAV
-> Sarvam Saaras batch REST API -> raw/final transcript + diagnostics
-> Windows clipboard -> restore original window -> Ctrl+V
```

## What this is

Voca is a Windows-first prototype. Hold Right Alt, speak, and release it to send
the captured audio to Sarvam Saaras. Voca shows the raw API transcript separately
from optional post-processing, reports per-dictation diagnostics, and attempts to
paste the final output into the previously focused Windows application.

The language selector exposes the language codes configured for Saaras, including
an `unknown` auto-detect choice. Actual recognition quality and language behavior
come from the configured Sarvam model and are not guaranteed by Voca.

## What this isn't

- Not a full keyboard replacement
- Not a translation tool (see [sarvam-translate](https://github.com/sovereign-shovels/sarvam-pdf))
- No offline or local transcription provider
- No streaming transcription
- No macOS or Linux support in the current implementation

See [PRD-v1.md](./PRD-v1.md) for the full anti-scope definition.

---

## Install

### Pre-built binaries

Download the latest Windows installer from the
[Voca releases page](https://github.com/ananyap1119/voca/releases).

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
# %APPDATA%\voca\config.toml
[provider]
endpoint = "https://api.sarvam.ai/speech-to-text"
api_key_env_var = "SAARAS_API_KEY"
language = "hi-IN"
codemix = true
```

**Configured language choices:** `unknown`, `hi-IN`, `as-IN`, `bn-IN`, `brx-IN`,
`doi-IN`, `en-IN`, `gu-IN`, `kn-IN`, `kok-IN`, `ks-IN`, `mai-IN`, `ml-IN`,
`mni-IN`, `mr-IN`, `ne-IN`, `od-IN`, `pa-IN`, `sa-IN`, `sat-IN`, `sd-IN`,
`ta-IN`, `te-IN`, `ur-IN`.

**Codemix:** When enabled, Voca sends `mode=codemix` instead of
`mode=transcribe`. Voca does not implement language switching locally.

### Environment variables

All config options can be set via env vars (prefix: `VOCA_`):

```bash
export VOCA_LANGUAGE="ta-IN"
export VOCA_CODEMIX="true"
export VOCA_HOTKEY="RightAlt"
```

When upgrading an existing installation, replace the previous app-specific
environment-variable prefix with `VOCA_`. Provider credentials such as
`SAARAS_API_KEY` keep their provider-specific names.

### Changing the hotkey

Default on Windows: hold `Right Alt` while speaking and release it to transcribe.
`F8` is also accepted by the existing low-level hook.

```toml
# %APPDATA%\voca\config.toml
[provider]
hotkey = "RightAlt"
```

---

## Usage

1. Install and launch Voca.
2. Paste your Sarvam API key in Voca and select **Save key**.
3. Keep Polish set to **Off** when evaluating raw Saaras output.
4. Focus any text field, then hold `Right Alt` while speaking.
5. Release `Right Alt`. Voca transcribes and pastes the final output into the
   previously focused application.

Long dictation is split into API-safe parts automatically. Each dictation uses a
unique temporary directory that is removed after success or failure; Voca does
not keep an audio or transcript history.

Click the tray icon to open Settings and change the language or codemix mode.

The ordinary Rust test suite covers configuration, recording lifecycle state,
audio chunking, transcript assembly/cleanup, and request modes. Hardware and real
Sarvam smoke tests are present but ignored by default because they require a
microphone or API key.

---

## Why this exists

Indic dictation on macOS, Windows, and Linux is genuinely broken. Apple's dictation for Hindi/Tamil/Telugu has been bad for years. Google's only works in Chrome. Indians who think and write in their first language type slower than they think — and that's a quality-of-life problem at scale.

See [PRD-v1.md](./PRD-v1.md) for the full problem statement and rationale.

## Current scope

This repository currently targets reliable Windows push-to-talk dictation and
local, in-memory diagnostics for Saaras evaluation. It does not yet include a
benchmark dataset, persistent result export, telemetry, streaming, or local STT.

---

## License

Apache 2.0. See [LICENSE](./LICENSE).

## Part of sovereign-shovels

This repo is part of the [sovereign-shovels](https://github.com/sovereign-shovels) portfolio of small, focused, sovereign-by-construction AI utilities.

Other shovels: claude-vault, bulbul-studio, voca, claude-prompts, ollama-cron, mcp-forge, sarvam-pdf, agent-console, sarvam-meet, obsidian-llm, llm-diff, claude-bridge, claude-radio, sarvam-cast.
