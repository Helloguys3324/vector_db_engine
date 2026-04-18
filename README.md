<div align="center">

# 🛡️ Gemini Moderation Stack

**Hybrid Telegram message moderation engine**

[![Rust](https://img.shields.io/badge/Rust-stable-orange?logo=rust)](https://www.rust-lang.org/)
[![Python](https://img.shields.io/badge/Python-3.10+-blue?logo=python)](https://www.python.org/)
[![Node.js](https://img.shields.io/badge/Node.js-18+-green?logo=node.js)](https://nodejs.org/)
[![Qdrant](https://img.shields.io/badge/Qdrant-vector_db-red?logo=qdrant)](https://qdrant.tech/)
[![Django](https://img.shields.io/badge/Django-dashboard-darkgreen?logo=django)](https://www.djangoproject.com/)
[![CI](https://github.com/Helloguys3324/vector_db_engine/actions/workflows/build-and-release.yml/badge.svg)](https://github.com/Helloguys3324/vector_db_engine/actions/workflows/build-and-release.yml)

</div>

---

## Overview

Gemini Moderation Stack is a two-layer moderation pipeline for Telegram chats:

| Layer | Tech | What it does |
|---|---|---|
| **L1 — Lexical** | Rust, Aho-Corasick, fuzzy matching, heuristic decision layer | Fast deterministic filtering (profanity, obfuscation, risky lexical patterns) |
| **L2 — Semantic** | ONNX, Qdrant | Context-aware checks (semantic profanity/scam edge cases) when L1 is inconclusive |

The design is **cost-aware**: cheap deterministic checks first, expensive vector checks only when necessary.

---

## Architecture

```text
Telegram Update
   │
   ▼
telegram_bot (main.rs)
   │
   ▼
ModerationEngine::check_payload (vector_db_engine::lib.rs)
   │
   ├─ SIMD normalization + candidate generation
   ├─ L1 lexical skip / DFA / parity decision layer
   ├─ Pre-L2 contextual whitelist phrase block
   ├─ L2 profanity-candidate semantic probe
   └─ L2 fallback semantic scan (gated)
   │
   ▼
ALLOW ✅  or  BLOCK 🚫
   │
   └─ Bot runtime action: leave message OR delete + warning
```

---

## Full Message Lifecycle (actual runtime path)

This section describes the **real message execution flow** in the current codebase.

### A) Intake and routing (`telegram_bot\src\main.rs`)

1. Telegram dispatcher receives update and filters for message updates.
2. Non-text messages are ignored.
3. If text starts with `/train `, the message goes to dynamic semantic training path.
4. In `/train` mode, text is embedded and upserted into Qdrant with category `live_trained_profanity`.
5. For normal messages, bot calls `engine.check_payload(text).await`.

### B) Trace and token staging (`vector_db_engine\src\lib.rs`)

6. If `OMEGA_TRACE_WORD_PIPELINE` is enabled (default), engine logs:
   - incoming payload
   - per-word transforms: `raw -> strict -> collapsed`
7. Engine creates `SimdBuffer` and runs adversarial normalization.

### C) SIMD normalization and candidate generation (`simd_preprocessor.rs`)

8. Input is normalized with Unicode NFKC and lowercased.
9. Character mapping handles leet/symbol forms:
   - digits mapped conditionally (`4->a`, `3->e`, `1->i`, etc.)
   - symbols mapped (`@->$a`, `$->s`, `!|->i`, `+->t`)
10. Two candidate streams are produced:
   - **strict** (keeps repeated letters)
   - **collapsed** (repeat-collapsed variant)
11. Each token is scored for obfuscation signals:
   - non-alpha chars, repeated runs, consonant-heavy shapes
12. Canonical obfuscation cleanup can emit extra variants (example: `pusssie -> pussy`).
13. Short chunk-merging builds joined obfuscations from split text (example: `n i g g a -> nigga`).
14. Candidate sets are merged into a deduplicated **merged** list (bounded by max candidate limits).

### D) Early lexical fast-exit (`js_parity.rs`)

15. Engine evaluates `lexical_skip_reason`:
   - only single-token cases are eligible
   - semantic trigger tokens disable this fast-skip
   - whitelist or known-clean lexicon+BLOOM hit can short-circuit to ALLOW
16. If skip triggers, engine returns ALLOW immediately.

### E) Raw lexical detection + parity decision layer

17. Native DFA raw scan runs on both:
   - original payload
   - normalized-with-spaces payload
18. `JsParityEngine::analyze` is executed (with optional analysis cache for repeated texts).
19. Surface signals are extracted (`digit`, `leet`, `hard_separator`, `hyphen_only`, `apostrophe`, `alpha_only`).
20. High-risk phrases (`kill yourself`, `just die`, etc.) can immediately hard-block.
21. Native hit validation checks whether lexical hit corresponds to bad-word candidates not overridden by whitelist.
22. Short acronym subsystem evaluates aggressive short profane acronyms.
23. Heuristic evidence collection computes:
   - exact lexical hit
   - fuzzy strong/weak Damerau-Levenshtein signals
   - consonant skeleton proximity
   - obfuscation and clean-word evidence
24. Context guards suppress false positives in date/code/math/numeric-noise-like inputs.
25. Decision model applies weighted scoring (`linear -> sigmoid -> allow/review/block` thresholds).
26. If lexical decision is final, engine returns ALLOW/BLOCK without touching semantic fallback.

### F) Pre-L2 contextual phrase guard

27. Before broad semantic fallback, contextual phrase checker can block if:
   - message contains semantic trigger tokens
   - all tokens are clean/whitelisted words
   - bad-word set has no direct hit
   - phrase matches contextual whitelist phrase dataset
28. This catches clean-word toxic intent patterns prior to vector search.

### G) L2 semantic profanity candidate probe (`l2_semantic.rs`)

29. Engine extracts profanity vector candidates from merged obfuscated candidates (up to configured caps).
30. For each candidate:
   - tokenize + ONNX embedding
   - Qdrant search with category filter (`seed_profanity` or `live_trained_profanity`)
   - compare against profanity semantic threshold (`OMEGA_PROFANITY_VECTOR_THRESHOLD`, default `0.80`)
31. Any candidate semantic hit returns BLOCK.

### H) L2 semantic fallback gate (broad context scan)

32. If still unresolved, fallback gate checks:
   - vector fallback enabled
   - semantic trigger tokens/phrases OR
   - text length/token thresholds + risk markers (link/money/urgency)
   - excludes some noisy obfuscation surfaces for this stage
33. If gate opens, full message embedding is searched in Qdrant with general threshold (default `0.65`).
34. Semantic hit => BLOCK, miss => ALLOW.

### I) Degraded-mode behavior and bot action

35. If ONNX/tokenizer is unavailable, engine runs in **L1-only mode** (no panic).
36. Final decision returns to bot runtime:
   - `ALLOW`: leave message
   - `BLOCK`: delete message, send warning, delete warning after 5 seconds

### Representative trace markers

```text
[word:1] raw='...' -> strict='...' -> collapsed='...'
[simd] strict(count=...)
[l1-skip] true|false
[dfa] native_raw_hit=...
[parity] matched=... score=...
[pre-l2.context] matched contextual whitelist phrase '...'
[l2.profanity] candidates(count=...)
[l2.fallback] enabled=true
[final] decision=BLOCK reason=semantic_fallback
```

---

## Repository Map

```text
gemini/
├── Cargo.toml
├── rust_dict.txt
├── vector_db_engine/
│   ├── src/
│   │   ├── lib.rs
│   │   ├── dfa_fast_path.rs
│   │   ├── simd_preprocessor.rs
│   │   ├── js_parity.rs
│   │   ├── l2_semantic.rs
│   │   └── disruptor.rs
│   └── models/
│       ├── model_quantized.onnx
│       └── tokenizer.json
├── telegram_bot/
│   └── src/main.rs
├── profanity-destroyer/
│   ├── src/database/moderation-db.json
│   ├── scripts/word-admin.mjs
│   └── Largest.list.of.english.words.txt
├── scripts/python/
│   ├── build_rust_dictionary.py
│   ├── mine_moderation_data.py
│   ├── seed_qdrant_from_hf.py
│   ├── seed_qdrant_from_local_datasets.py
│   ├── seed_qdrant_nitro_examples.py
│   └── seed_qdrant_from_mined_toxicity.py
├── dashboard/
│   ├── channels_app/
│   └── templates/dashboard/
└── not used/
```

---

## Quick Start

### Prerequisites

- Rust `stable-x86_64-pc-windows-msvc`
- Visual Studio Build Tools with C++ workload (`link.exe` required)
- Python 3.10+
- Node.js 18+
- Docker (for Qdrant)

### 1) Install JS dependencies

```powershell
npm --prefix profanity-destroyer install
```

### 2) Configure environment

Create `.env` in repo root:

```env
BOT_TOKEN=YOUR_TELEGRAM_BOT_TOKEN
DETECTOR_MODE=balanced
```

### 3) Build lexical dictionary

```powershell
python scripts\python\build_rust_dictionary.py
```

### 4) Start Qdrant

```powershell
docker run -p 6333:6333 -p 6334:6334 qdrant/qdrant
```

Python seed scripts use `6333`, Rust bot currently uses `6334`.

### 5) Place semantic model assets (recommended)

Put files under `vector_db_engine\models\`:

- `model_quantized.onnx`
- `tokenizer.json`

Or set explicit paths:

- `OMEGA_MODEL_PATH`
- `OMEGA_TOKENIZER_PATH`

If these files are missing, bot starts in L1-only mode.

### 6) Optional: seed vector DB

```powershell
python scripts\python\mine_moderation_data.py --max-contexts 100000 --max-per-dataset 300000 --min-generic-toxic-score 0.5
python scripts\python\seed_qdrant_from_hf.py
python scripts\python\seed_qdrant_from_local_datasets.py --csv .\datasets\discord-phishing-scam-detection.csv --xlsx .\datasets\testing.xlsx
python scripts\python\seed_qdrant_nitro_examples.py
python scripts\python\seed_qdrant_from_mined_toxicity.py --input .\datasets\mined_toxicity.json
```

Optional contrastive mining:

```powershell
python scripts\python\mine_moderation_data.py --enable-lmsys-triplets --lmsys-max-triplets 50000 --triplets-output .\datasets\mined_toxic_triplets.jsonl
```

### 7) Run bot

```powershell
cargo run -p telegram_bot
```

---

## Environment Variables

### Telegram Bot

| Variable | Description | Default |
|---|---|---|
| `BOT_TOKEN` | Telegram bot token | required |
| `DETECTOR_MODE` | Runtime mode hint | `balanced` |
| `OMEGA_RUST_DICT_PATH` | Path to `rust_dict.txt` | auto |
| `OMEGA_MODEL_PATH` | Path to ONNX model | auto |
| `OMEGA_TOKENIZER_PATH` | Path to tokenizer | auto |

### Engine & Semantics

| Variable | Description | Default |
|---|---|---|
| `OMEGA_PROFANITY_VECTOR_SEED_LIMIT` | Number of profanity terms seeded into Qdrant | `2000` |
| `OMEGA_PROFANITY_VECTOR_THRESHOLD` | Candidate semantic threshold | `0.80` |
| `OMEGA_CONTEXT_PHRASE_PATH` | Contextual phrase file | auto |
| `OMEGA_PROFANITY_ROOT` | Path to `profanity-destroyer` assets | auto |
| `OMEGA_EXCLUDE_LEGACY_EXTERNAL` | `1` disables legacy external dictionary | off |
| `OMEGA_TRACE_WORD_PIPELINE` | Trace logs (`0`/`off` disables) | on |

### Django Dashboard

| Variable | Description |
|---|---|
| `DASHBOARD_TELEGRAM_BOT_TOKEN` | Telegram bot token for dashboard auth |
| `DASHBOARD_TELEGRAM_BOT_USERNAME` | Optional bot username (auto-resolved via `getMe` if empty) |

---

## Django Dashboard

```powershell
Set-Location .\dashboard
python manage.py migrate
python manage.py runserver
```

| URL | Description |
|---|---|
| `http://127.0.0.1:8000/` | Landing page with Telegram login |
| `http://127.0.0.1:8000/dashboard/` | Channel dashboard (auth required) |

Dashboard access is Telegram-authenticated, and channel visibility/editability is scoped to chats where the user is `administrator` or `creator`.

---

## Dictionary Management UI

```powershell
npm --prefix profanity-destroyer run word-admin
```

Open: `http://127.0.0.1:3210`

Features:

- Manage whitelist and custom blacklist
- Review unified moderation database
- Trigger dictionary rebuild (`rust_dict.txt`)

Runtime whitelist sources used by `telegram_bot`:

- `profanity-destroyer/src/database/moderation-db.json` → `whitelist`
- `profanity-destroyer/src/database/whitelist.txt` (line-based whitelist words)

---

## Troubleshooting

| Problem | Fix |
|---|---|
| `link.exe not found` | Install Visual Studio Build Tools with C++ workload, reopen shell from VS dev environment |
| `model_quantized.onnx does not exist` | Put model/tokenizer into `vector_db_engine\models\` or set explicit env paths |
| Telegram Login: `Bot domain invalid` | Configure BotFather `/setdomain` to the exact HTTPS domain serving dashboard |
| Dashboard shows no channels | Ensure bot is in target chats and your Telegram user is admin/creator there |
| Logs are too noisy | Set `OMEGA_TRACE_WORD_PIPELINE=0` |

---

## Development

```powershell
cargo fmt --all
cargo check --workspace
cargo test --workspace
node --check profanity-destroyer\scripts\word-admin.mjs
python -m py_compile scripts\python\build_rust_dictionary.py
```

---

## Runtime Training

Send `/train <text>` in moderated chat to upsert a new semantic sample into Qdrant (`live_trained_profanity` category) without restarting bot.

---

## Known Quirks

- Python seeding scripts and Rust runtime currently target different default Qdrant ports (`6333` vs `6334`).
- Some historical artifacts remain in `not used\` and are not part of active runtime.
