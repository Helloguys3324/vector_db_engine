<div align="center">

# 🛡️ Gemini Moderation Stack

**Hybrid Telegram message moderation engine**  
**Гибридный движок модерации сообщений для Telegram**

[![Rust](https://img.shields.io/badge/Rust-stable-orange?logo=rust)](https://www.rust-lang.org/)
[![Python](https://img.shields.io/badge/Python-3.10+-blue?logo=python)](https://www.python.org/)
[![Node.js](https://img.shields.io/badge/Node.js-18+-green?logo=node.js)](https://nodejs.org/)
[![Qdrant](https://img.shields.io/badge/Qdrant-vector_db-red?logo=qdrant)](https://qdrant.tech/)
[![Django](https://img.shields.io/badge/Django-dashboard-darkgreen?logo=django)](https://www.djangoproject.com/)

---

*[English](#english) · [Русский](#русский)*

</div>

---

<a name="english"></a>

## 🇬🇧 English

### Overview

A two-layer hybrid moderation pipeline that protects Telegram chats from profanity, obfuscations, scam, and toxic content:

| Layer | Technology | Purpose |
|-------|-----------|---------|
| **L1 — Lexical** | Rust · Aho-Corasick · Damerau-Levenshtein | Lightning-fast detection of profanity and obfuscations |
| **L2 — Semantic** | ONNX · Qdrant | Vector-based detection of scam and complex edge cases |

---

### Architecture

```
Telegram Message
       │
       ▼
  ┌─────────────────────────────────────────┐
  │         ModerationEngine                │
  │                                         │
  │  1. SimdPreprocessor                    │
  │     NFKC · leet-fold · candidates       │
  │                                         │
  │  2. Early Skip (js_parity)              │
  │     whitelist · clean-lexicon (100k+)   │
  │                                         │
  │  3. L1 — Lexical                        │
  │     Aho-Corasick DFA · fuzzy match      │
  │     decision layer · acronym guards     │
  │                                         │
  │  4. L2 — Semantic (if L1 inconclusive)  │
  │     ONNX embedding · Qdrant ANN search  │
  │     profanity candidates · scam scan    │
  └─────────────────────────────────────────┘
       │
       ▼
  CLEAN ✅  or  VIOLATION 🚫
  (bot warns + deletes message)
```

---

### Repository Map

```
gemini/
├── Cargo.toml                        # Rust workspace
├── rust_dict.txt                     # L1 flat dictionary
│
├── vector_db_engine/                 # Core hybrid engine (Rust library)
│   ├── src/
│   │   ├── lib.rs                    # L1/L2 orchestrator
│   │   ├── dfa_fast_path.rs          # Aho-Corasick + fuzzy
│   │   ├── simd_preprocessor.rs      # Normalization & obfuscation candidates
│   │   ├── js_parity.rs              # Decision layer + gates
│   │   ├── l2_semantic.rs            # ONNX embedding + Qdrant
│   │   ├── disruptor.rs              # Lock-free ring buffer
│   │   └── embedded_js/              # Compile-time fallback JSONs
│   └── models/
│       ├── model_quantized.onnx
│       └── tokenizer.json
│
├── telegram_bot/                     # Telegram runtime (Rust binary)
│   └── src/main.rs                   # Polling loop + /train command
│
├── profanity-destroyer/              # Dictionary management (JS)
│   ├── src/database/moderation-db.json  # Unified word DB
│   ├── scripts/word-admin.mjs        # Admin UI
│   └── Largest.list.of.english.words.txt
│
├── scripts/python/
│   ├── build_rust_dictionary.py      # Builds rust_dict.txt
│   ├── mine_moderation_data.py       # Mines toxic data from HF datasets
│   ├── seed_qdrant_from_hf.py        # Seeds Qdrant from HuggingFace
│   ├── seed_qdrant_from_local_datasets.py
│   ├── seed_qdrant_nitro_examples.py
│   └── seed_qdrant_from_mined_toxicity.py
│
├── dashboard/                        # Django web dashboard
│   ├── channels_app/                 # Channel models & views
│   └── templates/dashboard/
│
└── not used/                         # Archived / legacy artifacts
```

---

### Quick Start

#### Prerequisites

- **Rust** — `stable-x86_64-pc-windows-msvc`
- **Visual Studio Build Tools** with C++ (requires `link.exe`)
- **Python** 3.10+
- **Node.js** 18+
- **Docker** (for Qdrant)

#### 1 — Install dependencies

```powershell
npm --prefix profanity-destroyer install
```

#### 2 — Configure environment

Create `.env` in the repo root:

```env
BOT_TOKEN=YOUR_TELEGRAM_BOT_TOKEN
DETECTOR_MODE=balanced
```

#### 3 — Build the dictionary

```powershell
python scripts\python\build_rust_dictionary.py
```

#### 4 — Start Qdrant

```powershell
docker run -p 6333:6333 -p 6334:6334 qdrant/qdrant
```

> ⚠️ **Port note:** Python seed scripts default to `6333`; the Rust bot connects to `6334`. Keep both ports exposed, or unify the URL in the source.

#### 5 — (Optional) Seed the vector database

```powershell
python scripts\python\mine_moderation_data.py --max-contexts 100000 --max-per-dataset 300000 --min-generic-toxic-score 0.5
python scripts\python\seed_qdrant_from_hf.py
python scripts\python\seed_qdrant_from_local_datasets.py --csv .\datasets\discord-phishing-scam-detection.csv --xlsx .\datasets\testing.xlsx
python scripts\python\seed_qdrant_nitro_examples.py
python scripts\python\seed_qdrant_from_mined_toxicity.py --input .\datasets\mined_toxicity.json
```

For contrastive triplet mining (requires HF access token):

```powershell
python scripts\python\mine_moderation_data.py --enable-lmsys-triplets --lmsys-max-triplets 50000 --triplets-output .\datasets\mined_toxic_triplets.jsonl
```

#### 6 — Run the bot

```powershell
cargo run -p telegram_bot
```

---

### Dictionary Management UI

```powershell
npm --prefix profanity-destroyer run word-admin
# Open → http://127.0.0.1:3210
```

The UI allows you to:
- Add/remove **whitelist** and custom **blacklist** entries
- View the current unified word database
- Trigger a **Rebuild `rust_dict.txt`** with one click

---

### Environment Variables

#### Telegram Bot

| Variable | Description | Default |
|----------|-------------|---------|
| `BOT_TOKEN` | Telegram bot token | **required** |
| `DETECTOR_MODE` | Detection mode hint | `balanced` |
| `OMEGA_RUST_DICT_PATH` | Explicit path to `rust_dict.txt` | auto |
| `OMEGA_MODEL_PATH` | Explicit path to `.onnx` model | auto |
| `OMEGA_TOKENIZER_PATH` | Explicit path to `tokenizer.json` | auto |

#### Engine & Semantics

| Variable | Description | Default |
|----------|-------------|---------|
| `OMEGA_PROFANITY_VECTOR_SEED_LIMIT` | Profanity terms loaded into Qdrant | `2000` |
| `OMEGA_PROFANITY_VECTOR_THRESHOLD` | Semantic similarity threshold | `0.80` |
| `OMEGA_CONTEXT_PHRASE_PATH` | Path to contextual whitelist phrases | auto |
| `OMEGA_PROFANITY_ROOT` | Explicit path to `profanity-destroyer/` | auto |
| `OMEGA_EXCLUDE_LEGACY_EXTERNAL` | Set `1` to disable legacy external dict | off |
| `OMEGA_TRACE_WORD_PIPELINE` | Console trace per message/token | on |

#### Django Dashboard

| Variable | Description |
|----------|-------------|
| `DASHBOARD_TELEGRAM_BOT_TOKEN` | Bot token for Telegram login |
| `DASHBOARD_TELEGRAM_BOT_USERNAME` | Bot username (without `@`) |

---

### Django Dashboard

```powershell
Set-Location .\dashboard
python manage.py migrate
python manage.py runserver
```

| URL | Description |
|-----|-------------|
| `http://127.0.0.1:8000/` | Landing page with Telegram login |
| `http://127.0.0.1:8000/dashboard/` | Channel overview (auth required) |
| `http://127.0.0.1:8000/admin/` | Django admin panel |

Access is scoped: only channels where the authenticated user holds `administrator` or `creator` status are shown and editable.

---

### Development

```powershell
# Rust
cargo fmt --all
cargo check --workspace
cargo test --workspace

# JS
node --check profanity-destroyer\scripts\word-admin.mjs

# Python
python -m py_compile scripts\python\build_rust_dictionary.py
```

> If you see `link.exe not found` — install **Visual Studio Build Tools** with the C++ workload.

---

### Runtime Training

Send `/train <text>` in any moderated chat to add a new embedding directly into Qdrant's `live_trained_profanity` collection without restarting the bot.

---

### Known Quirks

- **Qdrant port mismatch:** seed scripts use `6333`, Rust bot uses `6334` — expose both or patch the code.
- `build_rust_dictionary.py` resolves all paths relative to its own location — safe to run from any working directory.
- `seed_qdrant_from_local_datasets.py` accepts `--csv`/`--xlsx` flags or `LOCAL_SCAM_CSV_PATH`/`LOCAL_SCAM_XLSX_PATH` env vars — no hard-coded paths.
- Several historical/experimental files live in `not used/` and are excluded from the main runtime.

---

---

<a name="русский"></a>

## 🇷🇺 Русский

### Обзор

Двухуровневый гибридный пайплайн модерации для защиты Telegram-чатов от мата, обфускаций, скама и токсичного контента:

| Уровень | Технологии | Назначение |
|---------|-----------|-----------|
| **L1 — Лексический** | Rust · Aho-Corasick · Damerau-Levenshtein | Быстрое обнаружение мата и обфускаций |
| **L2 — Семантический** | ONNX · Qdrant | Векторная детекция скама и сложных случаев |

---

### Архитектура

```
Сообщение Telegram
       │
       ▼
  ┌─────────────────────────────────────────┐
  │         ModerationEngine                │
  │                                         │
  │  1. SimdPreprocessor                    │
  │     NFKC · leet-fold · кандидаты        │
  │                                         │
  │  2. Ранний skip (js_parity)             │
  │     whitelist · clean-лексикон (100k+)  │
  │                                         │
  │  3. L1 — Лексический                   │
  │     Aho-Corasick DFA · fuzzy match      │
  │     decision layer · acronym guards     │
  │                                         │
  │  4. L2 — Семантический (если L1 = ?)   │
  │     ONNX embedding · Qdrant ANN search  │
  │     profanity кандидаты · скан скама    │
  └─────────────────────────────────────────┘
       │
       ▼
  CLEAN ✅  или  НАРУШЕНИЕ 🚫
  (бот предупреждает + удаляет сообщение)
```

---

### Карта репозитория

```
gemini/
├── Cargo.toml                        # Rust workspace
├── rust_dict.txt                     # Плоский словарь L1
│
├── vector_db_engine/                 # Основной гибридный движок (Rust library)
│   ├── src/
│   │   ├── lib.rs                    # Оркестратор L1/L2
│   │   ├── dfa_fast_path.rs          # Aho-Corasick + fuzzy
│   │   ├── simd_preprocessor.rs      # Нормализация и кандидаты обфускаций
│   │   ├── js_parity.rs              # Decision layer + gates
│   │   ├── l2_semantic.rs            # ONNX embedding + Qdrant
│   │   ├── disruptor.rs              # Lock-free ring buffer
│   │   └── embedded_js/              # Compile-time fallback JSON
│   └── models/
│       ├── model_quantized.onnx
│       └── tokenizer.json
│
├── telegram_bot/                     # Telegram runtime (Rust binary)
│   └── src/main.rs                   # Polling loop + команда /train
│
├── profanity-destroyer/              # Управление словарями (JS)
│   ├── src/database/moderation-db.json  # Единая словарная БД
│   ├── scripts/word-admin.mjs        # Мини-UI управления словами
│   └── Largest.list.of.english.words.txt
│
├── scripts/python/
│   ├── build_rust_dictionary.py      # Сборка rust_dict.txt
│   ├── mine_moderation_data.py       # Майнинг токсичных данных из HF
│   ├── seed_qdrant_from_hf.py        # Наполнение Qdrant из HuggingFace
│   ├── seed_qdrant_from_local_datasets.py
│   ├── seed_qdrant_nitro_examples.py
│   └── seed_qdrant_from_mined_toxicity.py
│
├── dashboard/                        # Django веб-дашборд
│   ├── channels_app/                 # Модели и вьюхи каналов
│   └── templates/dashboard/
│
└── not used/                         # Архивные / legacy артефакты
```

---

### Быстрый старт

#### Требования

- **Rust** — `stable-x86_64-pc-windows-msvc`
- **Visual Studio Build Tools** с C++ (нужен `link.exe`)
- **Python** 3.10+
- **Node.js** 18+
- **Docker** (для Qdrant)

#### 1 — Установить зависимости

```powershell
npm --prefix profanity-destroyer install
```

#### 2 — Настроить окружение

Создать `.env` в корне репозитория:

```env
BOT_TOKEN=YOUR_TELEGRAM_BOT_TOKEN
DETECTOR_MODE=balanced
```

#### 3 — Собрать словарь

```powershell
python scripts\python\build_rust_dictionary.py
```

#### 4 — Запустить Qdrant

```powershell
docker run -p 6333:6333 -p 6334:6334 qdrant/qdrant
```

> ⚠️ **Важно:** Python-скрипты используют порт `6333`, Rust-бот подключается к `6334`. Пробрасывай оба порта, либо унифицируй URL в коде.

#### 5 — (Опционально) Наполнить векторную базу

```powershell
python scripts\python\mine_moderation_data.py --max-contexts 100000 --max-per-dataset 300000 --min-generic-toxic-score 0.5
python scripts\python\seed_qdrant_from_hf.py
python scripts\python\seed_qdrant_from_local_datasets.py --csv .\datasets\discord-phishing-scam-detection.csv --xlsx .\datasets\testing.xlsx
python scripts\python\seed_qdrant_nitro_examples.py
python scripts\python\seed_qdrant_from_mined_toxicity.py --input .\datasets\mined_toxicity.json
```

Для контрастного обучения (нужен HF-токен, датасет gated):

```powershell
python scripts\python\mine_moderation_data.py --enable-lmsys-triplets --lmsys-max-triplets 50000 --triplets-output .\datasets\mined_toxic_triplets.jsonl
```

#### 6 — Запустить бота

```powershell
cargo run -p telegram_bot
```

---

### UI управления словарём

```powershell
npm --prefix profanity-destroyer run word-admin
# Открыть → http://127.0.0.1:3210
```

Что умеет UI:
- Добавлять / удалять записи в **whitelist** и кастомный **blacklist**
- Просматривать текущую единую словарную базу
- Запускать **Rebuild `rust_dict.txt`** одной кнопкой

---

### Переменные окружения

#### Telegram Bot

| Переменная | Описание | По умолчанию |
|-----------|---------|-------------|
| `BOT_TOKEN` | Токен Telegram-бота | **обязателен** |
| `DETECTOR_MODE` | Режим детекции | `balanced` |
| `OMEGA_RUST_DICT_PATH` | Явный путь к `rust_dict.txt` | авто |
| `OMEGA_MODEL_PATH` | Явный путь к `.onnx` модели | авто |
| `OMEGA_TOKENIZER_PATH` | Явный путь к `tokenizer.json` | авто |

#### Движок и семантика

| Переменная | Описание | По умолчанию |
|-----------|---------|-------------|
| `OMEGA_PROFANITY_VECTOR_SEED_LIMIT` | Кол-во profanity-термов в Qdrant | `2000` |
| `OMEGA_PROFANITY_VECTOR_THRESHOLD` | Порог семантического сходства | `0.80` |
| `OMEGA_CONTEXT_PHRASE_PATH` | Путь к контекстным whitelist-фразам | авто |
| `OMEGA_PROFANITY_ROOT` | Явный путь к `profanity-destroyer/` | авто |
| `OMEGA_EXCLUDE_LEGACY_EXTERNAL` | `1` — отключает legacy external словарь | выкл |
| `OMEGA_TRACE_WORD_PIPELINE` | Трассировка пайплайна в консоль | вкл |

#### Django Dashboard

| Переменная | Описание |
|-----------|---------|
| `DASHBOARD_TELEGRAM_BOT_TOKEN` | Токен бота для Telegram-логина |
| `DASHBOARD_TELEGRAM_BOT_USERNAME` | Юзернейм бота (без `@`) |

---

### Django Dashboard

```powershell
Set-Location .\dashboard
python manage.py migrate
python manage.py runserver
```

| URL | Описание |
|-----|---------|
| `http://127.0.0.1:8000/` | Лендинг с кнопкой входа через Telegram |
| `http://127.0.0.1:8000/dashboard/` | Обзор каналов (нужна авторизация) |
| `http://127.0.0.1:8000/admin/` | Панель администратора Django |

Доступ к каналам — только те чаты, где пользователь является `administrator` или `creator`.

---

### Разработка

```powershell
# Rust
cargo fmt --all
cargo check --workspace
cargo test --workspace

# JS
node --check profanity-destroyer\scripts\word-admin.mjs

# Python
python -m py_compile scripts\python\build_rust_dictionary.py
```

> Ошибка `link.exe not found` — установи **Visual Studio Build Tools** с компонентом C++.

---

### Обучение в рантайме

Отправь `/train <текст>` в любом модерируемом чате — новый вектор добавится в коллекцию `live_trained_profanity` в Qdrant без перезапуска бота.

---

### Известные особенности

- **Разные порты Qdrant:** seed-скрипты — `6333`, Rust-бот — `6334`. Пробрасывай оба или унифицируй URL.
- `build_rust_dictionary.py` разрешает пути относительно своего расположения — безопасно запускать из любой директории.
- `seed_qdrant_from_local_datasets.py` принимает `--csv`/`--xlsx` или `LOCAL_SCAM_CSV_PATH`/`LOCAL_SCAM_XLSX_PATH` — без привязки к конкретной машине.
- Исторические и экспериментальные файлы вынесены в `not used/` и не участвуют в основном runtime.

---

<div align="center">

Made with 🦀 Rust · 🐍 Python · 🟨 JavaScript

</div>
