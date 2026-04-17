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
├─ rust_dict.txt                   # Плоский словарь для L1
├─ scripts\
│  └─ python\
│     ├─ build_rust_dictionary.py          # Сборка rust_dict.txt из словарей
│     ├─ seed_qdrant_from_hf.py            # Первичное наполнение Qdrant скам-датасетами
│     ├─ seed_qdrant_from_local_datasets.py# Дозаливка локальных датасетов в Qdrant
│     ├─ seed_qdrant_nitro_examples.py     # Дозаливка Nitro scam-паттернов
│     └─ seed_qdrant_from_mined_toxicity.py# Загрузка mined_toxicity в live_trained_profanity
├─ not used\                       # Архив неучаствующих в пайплайне файлов
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
   ├─ Largest.list.of.english.words.txt
   ├─ scripts\word-admin.mjs       # Мини UI для управления unified БД
   └─ src\
      ├─ database\moderation-db.json # Единая словарная БД (entries + whitelist + slangMap)
```

Неиспользуемые учебные/артефактные файлы вынесены в `not used\`.
Legacy JS-рантайм и вспомогательные JS-скрипты также вынесены в `not used\profanity-destroyer-js\`.

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
| `scripts/python/build_rust_dictionary.py` | Python | Сборка `rust_dict.txt` |
| `scripts/python/seed_qdrant_*.py` | Python | Наполнение Qdrant данными для L2 |

---

## 4) Источники слов и приоритеты

### 4.1 Blacklist для `rust_dict.txt`
`scripts/python/build_rust_dictionary.py` объединяет:

1. `profanity-destroyer/src/database/moderation-db.json` (`entries[*].match` и `entries[*].word`)
2. `profanity-destroyer/node_modules/naughty-words/en.json`
3. `vector_db_engine/src/embedded_js/merged-external.json` (legacy расширения словаря)

Результат: `rust_dict.txt` (перезаписывается при rebuild).

### 4.2 Whitelist
Whitelist хранится в unified базе:

- `profanity-destroyer/src/database/moderation-db.json` -> поле `whitelist`

Whitelist **не должен** попадать в `rust_dict.txt` — он учитывается отдельной логикой skip/исключений в движке.

### 4.3 Дополнительные источники `js_parity`
`vector_db_engine/src/js_parity.rs` грузит:

- embedded fallback `moderation-db.json` (compile-time include),
- runtime `profanity-destroyer/src/database/moderation-db.json` (если найден),
- `profanity-destroyer/Largest.list.of.english.words.txt` (top clean words + bloom),
- decision model (`src/embedded_js/decision-model.json` + runtime override в `profanity-destroyer/src/config/decision-model.json`).

---

## 5) Конфигурация (env и runtime)

### 5.1 Telegram runtime

| Переменная | Где используется | Значение по умолчанию / смысл |
|---|---|---|
| `BOT_TOKEN` | `telegram_bot/src/main.rs` | обязателен |
| `DETECTOR_MODE` | `telegram_bot/src/main.rs` | `balanced` (сейчас в Rust-боте только логируется) |
| `OMEGA_RUST_DICT_PATH` | `telegram_bot/src/main.rs` | явный путь к `rust_dict.txt` |

### 5.2 Engine / Parity / Semantic

| Переменная | Где | Смысл |
|---|---|---|
| `OMEGA_PROFANITY_VECTOR_SEED_LIMIT` | `vector_db_engine/src/lib.rs` | сколько profanity-термов загружается в Qdrant (0..25000, default 2000) |
| `OMEGA_PROFANITY_VECTOR_THRESHOLD` | `vector_db_engine/src/l2_semantic.rs` | порог семантики для profanity-кандидатов (0.5..0.99, default 0.80) |
| `OMEGA_MODEL_PATH` | `telegram_bot/src/main.rs` | явный путь к `model_quantized.onnx` |
| `OMEGA_TOKENIZER_PATH` | `telegram_bot/src/main.rs` | явный путь к `tokenizer.json` |
| `OMEGA_PROFANITY_ROOT` | `vector_db_engine/src/js_parity.rs` | явный путь к `profanity-destroyer` |
| `OMEGA_EXCLUDE_LEGACY_EXTERNAL` | `vector_db_engine/src/js_parity.rs` | `1` отключает legacy external словарь (`merged-external`) |
| `OMEGA_TRACE_WORD_PIPELINE` | `vector_db_engine/src/lib.rs` | трассировка каждого сообщения и этапов слова в консоль (default: включено, `0/false/off/no` отключают) |

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
npm --prefix profanity-destroyer install
```

3. Сгенерировать словарь:

```powershell
python scripts\python\build_rust_dictionary.py
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
python scripts\python\mine_moderation_data.py --max-contexts 100000 --max-per-dataset 300000 --min-generic-toxic-score 0.5
python scripts\python\seed_qdrant_from_hf.py
python scripts\python\seed_qdrant_from_local_datasets.py --csv .\datasets\discord-phishing-scam-detection.csv --xlsx .\datasets\testing.xlsx
python scripts\python\seed_qdrant_nitro_examples.py
python scripts\python\seed_qdrant_from_mined_toxicity.py --input .\datasets\mined_toxicity.json
```

`mine_moderation_data.py` теперь проходит по нескольким HF источникам (Jigsaw mirrors, RealToxicityPrompts, Civil Comments, Davidson/SetFit, tweets-hate-speech-detection, Berkeley measuring-hate-speech) и берет токсичные контексты из каждого до `--max-per-dataset`.

Для контрастного обучения L2 можно дополнительно собирать triplets из LMSYS:

```powershell
python scripts\python\mine_moderation_data.py --enable-lmsys-triplets --lmsys-max-triplets 50000 --triplets-output .\datasets\mined_toxic_triplets.jsonl
```

`lmsys/lmsys-chat-1m` — gated dataset, поэтому нужен доступ на Hugging Face (и обычно `HF_TOKEN`).

### 6.5 Запуск бота (Rust)

```powershell
Set-Location <repo-root>
cargo run -p telegram_bot
```

---

## 7) Управление whitelist/blacklist через UI

Запуск:

```powershell
npm --prefix profanity-destroyer run word-admin
```

Открыть:

```text
http://127.0.0.1:3210
```

Что делает UI:

1. Добавляет/удаляет записи в `profanity-destroyer/src/database/moderation-db.json`:
   - `whitelist`,
   - custom blacklist entries (`source: "custom-blacklist"`).
2. Показывает текущие слова из unified базы.
3. Кнопка **Rebuild rust_dict.txt** запускает `scripts/python/build_rust_dictionary.py`.

---

## 8) Проверки и команды для разработки

### 8.1 Rust

```powershell
Set-Location <repo-root>
cargo fmt --all
cargo check --workspace
cargo test --workspace
```

Если ошибка `link.exe not found` — не установлен/не доступен MSVC linker.

### 8.2 JS / Python

```powershell
node --check profanity-destroyer\scripts\word-admin.mjs
python -m py_compile scripts\python\build_rust_dictionary.py
python -m py_compile scripts\python\seed_qdrant_from_hf.py
python -m py_compile scripts\python\seed_qdrant_from_local_datasets.py
python -m py_compile scripts\python\seed_qdrant_nitro_examples.py
python -m py_compile scripts\python\seed_qdrant_from_mined_toxicity.py
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
| `embedded_js/decision-model.json` | встроенный fallback конфиг decision-модели |

---

## 10) JS-проект `profanity-destroyer` (текущее использование)

1. Хранение словарных данных (`src/database/moderation-db.json`, `Largest.list.of.english.words.txt`).
2. Мини-интерфейс управления словами (`scripts/word-admin.mjs`).
3. Исторические JS runtime/скрипты перенесены в `not used\profanity-destroyer-js\`.

---

## 11) Известные особенности

1. `scripts/python/build_rust_dictionary.py` собирает пути относительно своего расположения (без hardcoded `D:\gemini\...`).
2. `scripts/python/seed_qdrant_from_local_datasets.py` принимает пути через `--csv/--xlsx` или env (`LOCAL_SCAM_CSV_PATH`, `LOCAL_SCAM_XLSX_PATH`) и не привязан к конкретной машине.
3. Порт Qdrant в разных частях проекта отличается (`6333` vs `6334`).
4. В репозитории есть несколько исторических/экспериментальных артефактов, не участвующих в основном runtime.

---

## 12) Минимальный quick start

```powershell
Set-Location <repo-root>
npm --prefix profanity-destroyer install
python scripts\python\build_rust_dictionary.py
docker run -p 6333:6333 -p 6334:6334 qdrant/qdrant
cargo run -p telegram_bot
```

Для управления словарем:

```powershell
npm --prefix profanity-destroyer run word-admin
```

