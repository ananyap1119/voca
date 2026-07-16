---
repo: voca
rank: 3
score: 0.81
sprint: 1
substrate_anchor: Saaras
build_estimate: "2–3 weeks for v0.1"
status: planned
---

# PRD v1.0 — voca

> **One-liner:** Tray-app dictation for 22 Indian languages. Codemix Hinglish included. Apple/Google dictation, but actually good.
>
> **Substrate:** Indian professionals, students, content workers — anyone abandoned by Apple/Google's poor Indic dictation
> **Launch channels:** Indian Twitter/X (with Hindi/Tamil/Telugu hashtags), r/India, r/IndiaTech, professional LinkedIn India
> **Build estimate (v0.1):** 2–3 weeks for v0.1

---

## What problem does this solve

Indic dictation on macOS, Windows, and Linux is genuinely broken. Apple's dictation for Hindi/Tamil/Telugu has been bad for years. Google's only works in Chrome. Indians who think and write in their first language type slower than they think — and that's a quality-of-life problem at scale. Saaras v3 is best-in-class for Indic STT including Hinglish/Tanglish codemix. voca puts it on a global hotkey.

## Why this is a shovel and not a product

Demand has been screaming for a decade. Sovereign by construction. Ships in two to three weeks. Massive scope-evolution potential into meeting capture, long-form note-taking, voice command surface.

---

## v0.1 — what ships

System tray app + global hotkey. Press hotkey, speak in any of 10 Indic languages or Hinglish, get text pasted into active app. Saaras v3 streaming via WebSocket. Codemix mode toggle. Works on macOS, Windows, Linux.

### Acceptance criteria for v0.1

A v0.1 release is publishable to GitHub when ALL of these are true:

- [ ] Core functionality described above works on the primary developer machine.
- [ ] At least one local-only configuration is documented and tested (no cloud required; deferred from v0.2 beta to v0.5).
- [ ] BYO endpoint / BYO key configuration is documented.
- [ ] README explains: what it is, who it's for, how to install, how to configure, what it doesn't do.
- [ ] LICENSE present (Apache 2.0 unless overridden).
- [ ] No hardcoded keys or vendor URLs anywhere.
- [ ] No telemetry / phone-home.
- [ ] At least one passing test for the main code path.
- [ ] CI green.
- [ ] AGENTS.md compliance reviewed.

## v0.5 — first major evolution

Continuous dictation mode for long-form writing. Custom vocabulary per language. Quick translation toggle.

## v1.0 — fuller scope

Meeting capture mode. Multi-speaker diarization. Voice command shortcuts.

---

## Architecture sketch

### Stack

Tauri (cross-platform tray + global hotkey support is decent in Rust). Bundled audio capture per OS. Saaras/Sarvam STT primary for v0.2 BYO-key beta; Whisper-Indic local fallback by v0.5.

### Provider abstraction

The shovel MUST expose a provider abstraction even if v0.1 only uses one
provider. Suggested shape:

```
interface Provider {
  name: string;
  endpoint: URL;
  apiKeyEnvVar: string;
  call(input: ProviderInput): Promise<ProviderOutput>;
}
```

The v0.2 beta defaults to a BYO-key Sarvam provider with no bundled secret. Local-only inference is deferred to v0.5, and the provider abstraction must keep that path open.

### Configuration

Configuration order of precedence (highest to lowest):

1. Command-line flags
2. Environment variables (prefix: `VOCA_*`)
3. User config file (`~/.config/voca/config.toml` on Linux/Mac, equivalent on Windows)
4. Default config (shipped, but never with secrets)

---

## Anti-scope (do NOT build)

Not a full keyboard replacement. Not a translation tool (that's sarvam-translate). No offline mode in v0.1 (Whisper-Indic comes in v0.5).

---

## Tombstone risk and mitigation

**Risk:** Apple or Google massively improving Indic dictation. Has been promised for years; low probability inside 90 days.

**Mitigation:** Ship fast (v0.1 in 2–3 weeks for v0.1). Build community early
(launch on Indian Twitter/X (with Hindi/Tamil/Telugu hashtags), r/India, r/IndiaTech, professional LinkedIn India). Even if upstream absorbs the feature, accumulated
stars and the community are the audience-build payoff.

**Kill signal:** Apple's next iOS/macOS release ships actually-good Indic dictation. Watch WWDC.

If the kill signal triggers, the maintainer must announce within one week and
either (a) refocus on a remaining gap, (b) merge gracefully into upstream if
they're receptive, or (c) mark the repo as archived with a clear pointer to the
replacement.

---

## Launch plan

### Pre-launch checklist

- [ ] Repo on GitHub at `github.com/ananyap1119/voca`
- [ ] README polished (see template in `_templates/`)
- [ ] At least 3 issues / discussions seeded (real ones, not placeholder)
- [ ] LICENSE, CODE_OF_CONDUCT, CONTRIBUTING present
- [ ] Demo asset (gif, screenshot, or short video — depending on category)
- [ ] First-launch post drafted for primary launch channel

### Day-1 launch

Post to: Indian Twitter/X (with Hindi/Tamil/Telugu hashtags), r/India, r/IndiaTech, professional LinkedIn India

Subject template (adjust per channel):
- Show HN: `Show HN: voca - Tray-app dictation for 22 Indian languages. Codemix Hinglish included. Apple/Google dictation, but actually good.`
- Reddit: `[OSS] Tray-app dictation for 22 Indian languages. Codemix Hinglish included. Apple/Google dictation, but actually good.` with full post explaining the gap and the build
- Twitter/X: thread leading with the demo gif

### Week-1 follow-up

- Respond to every issue and comment within 24h.
- Ship at least one bugfix release based on launch feedback.
- Cross-post to secondary channels.

### Month-1 review

- Assess star velocity and community formation.
- If kill signal triggered, follow tombstone protocol above.
- If trajectory is healthy, plan v0.5.

---

## Cross-references

- Constitution: [[AGENTS]]
- Public README: [[README]]
- Progress frontmatter: [[progress]]
- Internal knowledge graph: [[knowledge-graph]]
