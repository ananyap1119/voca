---
repo: voca
rank: 3
score: 0.81
sprint: 1
substrate_anchor: Saaras
status: testing
v01_acceptance_pct: 90
last_update: 2026-07-16
stars: 0
dependents: 0
---

# Progress - voca

The frontmatter above is what the root [[../PORTFOLIO]] view aggregates.
Update it as the build progresses.

## Status legend

- `planned` - PRD complete, no code yet
- `scaffolding` - repo set up, dependencies in place
- `building` - actively writing v0.1 code
- `testing` - v0.1 feature-complete, in test
- `ready-to-launch` - passes acceptance criteria, awaits launch
- `live` - published on GitHub
- `tombstone-watch` - kill signal triggered, evaluating
- `archived` - gracefully shut down

## Milestones

### v0.1
- [x] Repo initialized
- [x] Provider abstraction in place
- [ ] Local-only provider implemented and tested
- [x] Repo initialized
- [x] Provider abstraction in place
- [ ] Local-only provider implemented and tested
- [x] Core functionality on primary platform (audio capture, Saaras v3 STT, paste)
- [x] One passing test for main code path
- [x] CI green
- [x] README polished
- [ ] Acceptance criteria from [[PRD-v1]] satisfied
- [ ] Launched

### Post-launch (track if `live`)
- Stars: 0
- Dependents: 0
- Open issues: 0
- Discord/community presence: none yet

## Decision log

> Append entries here for any decisions that affect direction.
> Format: `YYYY-MM-DD - what - why`.

- 2026-05-10 - scaffolded from sovereign-shovels-vault - initial PRD imported
- 2026-05-10 - Tauri v2 app compiles - tray icon, global shortcut, audio capture, STT provider abstraction, clipboard paste all wired
- 2026-06-22 - settings persistence wired - config saves back to disk, provider updates live, Windows build skips installer bundling by default
- 2026-06-22 - dictation transcript surfaced in UI - generated text is now visible and copyable in the app, not only pasted into the focused window
- 2026-06-23 - startup window made visible on Windows - tray fallback was too easy to miss, so the main window now opens on launch for a reliable entry point
- 2026-06-24 - Windows reveal hardened - startup now forces the top-level window to the foreground after a short delay so the UI is actually visible
- 2026-06-24 - main window created manually - app now opens a visible 960x640 Tauri window directly instead of relying on the hidden auto-window
- 2026-06-24 - tray settings reveal hardened - Settings now uses the same window reveal path as startup so it can create/show the main window reliably
- 2026-06-24 - foreground activation hardened - reveal now attaches to the foreground thread before restoring the window, so tray clicks should actually raise the app
- 2026-06-24 - Saaras key input relaxed - API field now accepts either the env var name or a pasted key so dictation can run without shell env setup
- 2026-06-24 - cloud dictation enabled locally - Saaras API key is now available in the launch environment for the current run
- 2026-06-24 - API key fallback hardened - fresh launches now read `SAARAS_API_KEY` directly if the config field is empty, so Windows dictation no longer depends on a saved config file
- 2026-06-24 - release rebuilt and relaunched - tests passed and the Windows binary was restarted with the key in-process
- 2026-06-24 - main window recreated from Rust - startup now closes any stale main window and rebuilds the packaged `index.html` window so the app stops falling back to localhost
- 2026-06-25 - Sarvam v3 request fixed - dictation now sends `mode=codemix`/`mode=transcribe` instead of the invalid boolean `codemix` form field
- 2026-06-25 - recorder made Windows-format aware - audio capture now writes WAV headers using the actual input sample rate, channel count, and sample format
- 2026-06-25 - Saaras v3 language coverage expanded - UI now exposes auto-detect plus Sarvam's 22 documented BCP-47 language codes
- 2026-06-25 - Windows TLS hardened - Sarvam HTTP calls now use Rustls-backed reqwest to avoid Schannel revocation failures on this machine
- 2026-06-25 - Tauri build path verified - final app was built with `tauri build --no-bundle`, API smoke test returned HTTP 200, and the rebuilt app was launched
- 2026-06-25 - Sarvam network path fixed - reqwest now uses Rustls with native Windows roots plus system proxy support, resolving the UnknownIssuer/request-send failure
- 2026-06-25 - dictation state surfaced - UI now receives backend status when audio is recording and after a WAV is captured before upload
- 2026-06-25 - smoke tests added - ignored tests verify one-second microphone WAV capture and the real Sarvam Rust client against a generated silent WAV
- 2026-06-25 - config merge regression fixed - settings saves now preserve the hidden Sarvam endpoint/model/hotkey fields so longer dictations cannot wipe the request URL
- 2026-06-25 - dictation window extended - manual and hotkey dictation now capture 30 seconds of audio before sending to Sarvam
- 2026-06-25 - dictation recovery hardened - recording state now clears after paste failures, duplicate starts are guarded, and transcripts get local punctuation/spacing cleanup
- 2026-06-25 - Wispr-style polish layer added - transcripts now pass through local voice-command punctuation/paragraph cleanup, with optional configurable full grammar polish
- 2026-06-25 - disfluency cleanup added - light polish now removes common spoken fillers and repeated vague words across English/Hinglish/Kannada-style dictation
- 2026-06-25 - recording watchdog added - backend now tracks recording start time, auto-recovers stale locks, and exposes a Reset control for stuck recording state
- 2026-06-25 - fixed recording window removed - dictation now records until speech pauses instead of stopping at 30 seconds
- 2026-06-25 - push-to-talk stop added - holding Alt starts recording, releasing it or pressing Stop ends recording, and paste now targets the previously active window
- 2026-06-25 - Flow-style keyboard hook added - Windows now uses a low-level hold/release hook for Alt instead of the unreliable global shortcut plugin
- 2026-07-15 - voca rename completed - project-specific environment variables, documentation, repository metadata, and local paths now use the voca identity
- 2026-07-15 - BYO-key release path added - Windows credentials now protect user API keys, long recordings are chunked safely, and tagged builds publish an NSIS installer through GitHub Releases
- 2026-07-16 - BYO-key installer verified - Rust tests pass and an NSIS `v0.2.0` installer builds locally; public release remains blocked on the required local-only provider
- 2026-07-16 - provider defaults externalized - Sarvam endpoint and model moved to shipped configuration, and the unimplemented local placeholder now fails explicitly instead of returning fake transcript text

## Tombstone watch

- 2026-06-22 - settings persistence wired - config saves back to disk, provider updates live, Windows build skips installer bundling by default

What we're monitoring (from PRD-v1):

Apple's next iOS/macOS release ships actually-good Indic dictation. Watch WWDC.

Status: not triggered.
- 2026-05-10 - hardened against local Ollama - all CLI paths verified, compile+tests green
