# Gemini Moderation Stack

Гибридный проект модерации сообщений для Telegram:

1. **L1 (лексика):** быстрый Rust-пайплайн для детекта мата и обфускаций.
2. **L2 (семантика):** ONNX + Qdrant для семантического детекта (в первую очередь скам / сложные случаи).
3. **JS-ветка (`profanity-destroyer`):** исторический/параллельный движок, база слов и утилиты управления словарями.

---

## 1) Карта репозитория

```text
D:\gemini
├─ Cargo.toml                      # Rust workspace (vector_db_engine + telegram_bot)
├─ compile_dict.py                 # Сборка rust_dict.txt из словарей
├─ rust_dict.txt                   # Плоский словарь для L1
├─ ingest_scam.py                  # Первичное наполнение Qdrant скам-датасетами
├─ ingest_local.py                 # Дозаливка локальных датасетов в Qdrant
├─ ingest_nitro.py                 # Дозаливка Nitro scam-паттернов
├─ vector_db_engine\               # Rust-библиотека движка модерации
│  ├─ Cargo.toml
│  ├─ models\
│  │  ├─ model_quantized.onnx
│  │  └─ tokenizer.json
│  └─ src\
│     ├─ lib.rs                    # Orchestrator L1/L2
│     ├─ dfa_fast_path.rs          # Aho-Corasick + fuzzy (Damerau-Levenshtein)
│     ├─ simd_preprocessor.rs      # Нормализация/кандидаты обфускаций
│     ├─ js_parity.rs              # JS-паритет логики + decision layer
│     ├─ l2_semantic.rs            # ONNX embedding + Qdrant search/upsert
│     ├─ disruptor.rs              # Lock-free очередь (вспомогательный модуль)
│     ├─ embedded_js\              # Встроенные fallback JSON
│     └─ test_api.rs               # Мини smoke-файл для qdrant-client
├─ telegram_bot\
│  ├─ Cargo.toml
│  └─ src\main.rs                  # Telegram runtime + /train команда
└─ profanity-destroyer\            # Отдельный JS-проект (со своим .git)
   ├─ package.json
   ├─ bot.js                       # JS Telegram bot (legacy/альтернативный runtime)
   ├─ en.json                      # Основная кастомная profanity-база
   ├─ Largest.list.of.english.words.txt
   ├─ native\                      # NAPI Rust-модуль OmegaEngine (для JS)
   ├─ scripts\word-admin.mjs       # Мини UI для whitelist/blacklist
   └─ src\
      ├─ index.js                  # JS детектор
      ├─ database\
      │  ├─ index.js               # Агрегатор словарей
      │  ├─ whitelist.txt
      │  ├─ custom-blacklist.json
      │  └─ external\{abr.json, merged-external.json}
      ├─ vector\index.js           # JS semantic store
      └─ config\{decision-model.json, vector-semantic-db.json}
```

Дополнительно в корне есть **вспомогательные/учебные файлы** (`task_5_*.c`, `input.txt`, `output.txt`, `result.txt`, `data.json`, `experiment.txt`) — они не участвуют в основном пайплайне модерации.

---

## 2) Архитектура обработки сообщения (Rust runtime)

Поток (`telegram_bot/src/main.rs` -> `ModerationEngine::check_payload`):

1. Бот получает `msg.text()`.
2. Текст уходит в `vector_db_engine::ModerationEngine`.
3. `SimdBuffer` делает нормализацию + извлекает кандидаты (`strict/collapsed/merged`).
4. **Ранний skip (`js_parity.should_skip_lexical_stage`)**:
   - если токен в `whitelist` -> сразу clean,
   - если токен в топ-частотном clean-лексиконе (100k + bloom) и не обфусцирован -> сразу clean.
5. L1 lexical:
   - `dfa_fast_path`: raw Aho-Corasick + fuzzy,
   - `js_parity.analyze`: decision layer, short acronyms, guards, heuristic.
6. Если L1 не дал финал:
   - `scan_profanity_candidates` (семантическая проверка обфусцированных profanity-кандидатов),
   - затем `scan_semantic` (общий векторный fallback при выполнении условий).
7. Если violation=true, бот удаляет сообщение и шлет предупреждение.

Команда `/train <text>` добавляет новый вектор в Qdrant (`live_trained_profanity`) в рантайме.

---

## 3) Компоненты и роли

| Компонент | Язык | Роль |
|---|---|---|
| `telegram_bot` | Rust | Telegram polling runtime, вызовы движка, `/train` |
| `vector_db_engine` | Rust | Основной hybrid-движок L1 + L2 |
| `profanity-destroyer` | JS + Rust NAPI | Источник словарей, JS-референс, админка слов |
| `compile_dict.py` | Python | Сборка `rust_dict.txt` |
| `ingest_*.py` | Python | Наполнение Qdrant данными для L2 |

---

## 4) Источники слов и приоритеты

### 4.1 Blacklist для `rust_dict.txt`
`compile_dict.py` объединяет:

1. `profanity-destroyer/en.json`
2. `profanity-destroyer/src/database/custom-blacklist.json`
3. `profanity-destroyer/node_modules/naughty-words/en.json`

Результат: `rust_dict.txt` (перезаписывается при rebuild).

### 4.2 Whitelist
Whitelist хранится отдельно:

- `profanity-destroyer/src/database/whitelist.txt`

Whitelist **не должен** попадать в `rust_dict.txt` — он учитывается отдельной логикой skip/исключений в движке.

### 4.3 Дополнительные источники `js_parity`
`vector_db_engine/src/js_parity.rs` грузит:

- встроенные fallback JSON (`src/embedded_js/*.json`),
- `profanity-destroyer/en.json`,
- `profanity-destroyer/src/database/custom-blacklist.json`,
- `profanity-destroyer/src/database/external/merged-external.json` (если не отключено),
- `profanity-destroyer/src/database/external/abr.json`,
- `profanity-destroyer/src/database/whitelist.txt`,
- `profanity-destroyer/Largest.list.of.english.words.txt` (top clean words + bloom).

---

## 5) Конфигурация (env и runtime)

### 5.1 Telegram runtime

| Переменная | Где используется | Значение по умолчанию / смысл |
|---|---|---|
| `BOT_TOKEN` | `telegram_bot/src/main.rs` | обязателен |
| `DETECTOR_MODE` | `telegram_bot/src/main.rs` | `balanced` (сейчас в Rust-боте только логируется) |

### 5.2 Engine / Parity / Semantic

| Переменная | Где | Смысл |
|---|---|---|
| `OMEGA_PROFANITY_VECTOR_SEED_LIMIT` | `vector_db_engine/src/lib.rs` | сколько profanity-термов загружается в Qdrant (0..25000, default 2000) |
| `OMEGA_PROFANITY_VECTOR_THRESHOLD` | `vector_db_engine/src/l2_semantic.rs` | порог семантики для profanity-кандидатов (0.5..0.99, default 0.80) |
| `OMEGA_EXCLUDE_LEGACY_EXTERNAL` | `vector_db_engine/src/js_parity.rs` | `1` отключает `merged-external.json` |
| `OMEGA_PROFANITY_ROOT` | `vector_db_engine/src/js_parity.rs` | явный путь к `profanity-destroyer` |

### 5.3 Word Admin UI

| Переменная | Где | Смысл |
|---|---|---|
| `WORD_ADMIN_PORT` | `profanity-destroyer/scripts/word-admin.mjs` | порт UI (default `3210`) |

---

## 6) Установка и запуск (Windows, локально)

### 6.1 Требования

1. Rust toolchain (`stable-x86_64-pc-windows-msvc`).
2. Visual Studio Build Tools с C++ (нужен `link.exe`).
3. Python 3.10+.
4. Node.js 18+.
5. Qdrant (локально или Docker).

### 6.2 Подготовка

1. В корне проекта создать/обновить `.env`:

```env
BOT_TOKEN=YOUR_TELEGRAM_BOT_TOKEN
DETECTOR_MODE=balanced
```

2. Установить JS зависимости:

```powershell
npm --prefix D:\gemini\profanity-destroyer install
```

3. Сгенерировать словарь:

```powershell
python D:\gemini\compile_dict.py
```

### 6.3 Поднять Qdrant

Пример через Docker:

```powershell
docker run -p 6333:6333 -p 6334:6334 qdrant/qdrant
```

> Важно: ingestion-скрипты по умолчанию используют `6333`, а Rust-бот сейчас создает движок с URL `http://localhost:6334`.
> Держи порты согласованными (либо пробрасывай оба, либо унифицируй URL в коде).

### 6.4 (Опционально) Наполнить векторную базу

```powershell
python D:\gemini\ingest_scam.py
python D:\gemini\ingest_local.py
python D:\gemini\ingest_nitro.py
```

### 6.5 Запуск бота (Rust)

```powershell
Set-Location D:\gemini
cargo run -p telegram_bot
```

---

## 7) Управление whitelist/blacklist через UI

Запуск:

```powershell
npm --prefix D:\gemini\profanity-destroyer run word-admin
```

Открыть:

```text
http://127.0.0.1:3210
```

Что делает UI:

1. Добавляет/удаляет записи в:
   - `profanity-destroyer/src/database/whitelist.txt`
   - `profanity-destroyer/src/database/custom-blacklist.json`
2. Показывает текущие слова из файлов.
3. Кнопка **Rebuild rust_dict.txt** запускает `compile_dict.py`.

---

## 8) Проверки и команды для разработки

### 8.1 Rust

```powershell
Set-Location D:\gemini
cargo fmt --all
cargo check --workspace
cargo test --workspace
```

Если ошибка `link.exe not found` — не установлен/не доступен MSVC linker.

### 8.2 JS / Python

```powershell
node --check D:\gemini\profanity-destroyer\src\index.js
node --check D:\gemini\profanity-destroyer\scripts\word-admin.mjs
python -m py_compile D:\gemini\compile_dict.py
```

---

## 9) Детализация модулей `vector_db_engine`

| Файл | Что делает |
|---|---|
| `lib.rs` | orchestration L1/L2, ранний skip, vector probing |
| `dfa_fast_path.rs` | raw Aho-Corasick + fuzzy Damerau-Levenshtein |
| `simd_preprocessor.rs` | NFKC/leet folding, token candidates, merge strict/collapsed |
| `js_parity.rs` | полноценная decision-плоскость: whitelist, clean lexicon, short acronyms, heuristic, context guards, vector fallback gates |
| `l2_semantic.rs` | ONNX embedding, Qdrant search/upsert, bootstrap profanity vectors, `/train` integration |
| `disruptor.rs` | lock-free ring buffer для handoff (в текущем main-flow не центральный) |
| `embedded_js/*.json` | встроенные fallback словари и конфиг decision-модели |

---

## 10) JS-проект `profanity-destroyer` (зачем нужен здесь)

1. Источник/агрегатор словарей (`src/database/index.js`).
2. Исторический runtime и parity-эталон для переносов в Rust (`src/index.js`).
3. Нативный NAPI модуль (`native/`) для JS fast-path (`OmegaEngine`).
4. Инструменты:
   - `src/demo.js` — локальные демонстрационные тесты,
   - `scripts/word-admin.mjs` — UI словаря.

---

## 11) Известные особенности

1. `compile_dict.py` содержит **жестко заданные Windows-пути** (`D:\gemini\...`).
2. `ingest_local.py` ожидает конкретные локальные файлы в `C:\Users\PC\Downloads\...`.
3. Порт Qdrant в разных частях проекта отличается (`6333` vs `6334`).
4. В репозитории есть несколько исторических/экспериментальных артефактов, не участвующих в основном runtime.

---

## 12) Минимальный quick start

```powershell
Set-Location D:\gemini
npm --prefix profanity-destroyer install
python compile_dict.py
docker run -p 6333:6333 -p 6334:6334 qdrant/qdrant
cargo run -p telegram_bot
```

Для управления словарем:

```powershell
npm --prefix profanity-destroyer run word-admin
```

