# marola — run it locally, no Telegram, no Azure

A step-by-step guide to running marola's real pipeline end to end on your own machine — nearby
beach discovery, live sea conditions, and an LLM-generated summary reviewed by a second LLM pass —
with a small, fast Ollama model, so you can confirm the whole thing actually works before touching
Telegram or Azure at all. Every command below is real and was run against a live Ollama install
while building this (see `ARCHITECTURE.md` §5's "Status" notes) — the specific model recommended
here is deliberately smaller/faster than the one used to build/verify the rest of this repo
(`dolphin-mixtral:8x7b`, 26GB), chosen for this guide because it's cheap to download and quick to
try, not because it was the model marola was verified against everywhere else.

## 1. Prerequisites

- This repo cloned, with `nix develop` (or `direnv allow`) entered at least once — this puts JDK 25,
  sbt, `just`, and `ollama` itself on `PATH` (`flake.nix` was updated to include `pkgs.ollama` for
  exactly this guide).
- ~2GB free disk for the model below.

## 2. Start Ollama and pull a small model

```bash
# In one terminal, start the Ollama server (stays running in the foreground):
ollama serve
```

```bash
# In another terminal, pull a small, fast model — 1.3GB download, confirmed size:
ollama pull llama3.2:1b
```

Why this model specifically: `llama3.2:1b` is small enough to download in a couple of minutes on a
normal connection and fast enough on CPU alone to get a reply in seconds rather than the ~45
seconds a full-size model can take (confirmed against `dolphin-mixtral:8x7b`, a 26GB model, while
building this repo — see `ARCHITECTURE.md`). It's not the most capable model Ollama can run, but
"is the whole pipeline wired correctly end to end" doesn't need a capable model, just a working
one.

Verify the pull worked and Ollama is actually serving:

```bash
curl http://localhost:11434/api/tags
# {"models":[{"name":"llama3.2:1b", ...}]}
```

## 3. Point marola at it

`llama3.2:1b`'s Ollama tag is the `:1b` variant — marola's own default
(`LocalLlmClient.DefaultModel`) is the plain `llama3.2` tag (Ollama's 3B-parameter default), so
point at the small one explicitly for this guide:

```bash
export MAROLA_LOCAL_LLM_MODEL=llama3.2:1b
# Everything else can stay at its defaults — MAROLA_LLM_PROVIDER=local is already the default,
# and localhost:11434 is already LocalLlmClient's default base URL.
```

## 4. Run the pipeline — the actual E2E check

```bash
# Step 1: the deterministic pipeline alone — no LLM call yet, just Overpass + Open-Meteo +
# the scoring heuristic. This alone confirms network access and the core logic work.
just run

# Step 2: the full pipeline including both LLM passes — the actual "does the whole thing work
# end to end" check. This is the one command this whole guide is building up to.
just run -- --summarize
```

Expected shape of the output (your beach names/numbers will differ — this is live data):

```
marola :: best hour tomorrow to swim nearby (POC)
config -> AppConfig(...,Local,http://localhost:11434/v1,llama3.2:1b,...)
origin -> lat=-22.9878, lon=-43.1913 (radius 15km)
 1. [ 75/100] Praia do Diabo         (0.2km away)  best at Sat 5 Sep, 00:00  |  23.5°C sea, ...
 ...

Asking Local LLM to summarize the top pick (this may take a while)...
Draft summary: <whatever the model says>
Reviewer (score NN/100, verdict: approve|revise): <the reviewer's final text>
```

If you see a `Draft summary:` line followed by a `Reviewer (score .../100, ...)` line, the full
pipeline worked: beach discovery → conditions → scoring → summarization → review, all live, all
local, zero Azure.

## 5. The other two CLI paths, same local setup

These don't need anything beyond what §2-3 already set up:

```bash
# Record a sighting report (writes to ./data/sightings.jsonl):
just run -- --report-sighting jellyfish Arpoador "spotted near shore"

# Analyze a photo — needs a MULTIMODAL model, which llama3.2:1b is not (it's text-only).
# Pull one first if you want to try this path:
ollama pull llava
export MAROLA_LOCAL_VISION_MODEL=llava
just run -- --analyze-photo ./some-beach-photo.jpg
```

## 6. Troubleshooting

- **`HTTP 404 ... model 'X' not found`** — the model named in `MAROLA_LOCAL_LLM_MODEL` (or
  `MAROLA_LOCAL_VISION_MODEL`) isn't pulled. Run `ollama list` to see what you actually have, or
  `ollama pull <name>` to get it.
- **Connection refused to `localhost:11434`** — `ollama serve` isn't running, or isn't running in
  this same environment (e.g. a container that can't reach the host's Ollama). `flake.nix`'s
  `shellHook` checks for this and prints a reminder every time you enter the dev shell.
- **It's slow** — CPU-only inference is genuinely slow for bigger models; that's exactly why this
  guide recommends `llama3.2:1b` instead of whatever larger model you might already have pulled for
  other purposes. If it's still too slow, an even smaller model exists (e.g. `qwen2.5:0.5b`), at
  the cost of noticeably worse instruction-following — the reviewer pass in particular depends on
  the model reliably producing well-formed JSON (see `llm/Reviewer.scala`), which smaller models
  are more likely to get wrong.
- **The reviewer's JSON parsing fails** (`MalformedReviewException` or similar in the output) — a
  known, if infrequent, failure mode with smaller/quantized models that ignore the "respond with
  ONLY JSON" instruction (see `Reviewer.scala`'s `extractJsonObject` — it already recovers from a
  JSON block wrapped in prose, but a model that doesn't produce JSON *at all* isn't recoverable).
  Confirms `llama3.2:1b`'s instruction-following limits, not a marola bug — try a larger model if
  this happens consistently.

## 7. What this guide deliberately doesn't cover

Telegram bot setup (there is no bot loop yet — see `TELEGRAM-SETUP.md` for credential setup ahead
of that Phase 1 work) and any Azure integration (`ARCHITECTURE.md` §5/§6, all optional, none needed
for anything above). This guide is specifically the "prove it works, cheaply, before touching
anything else" path.
