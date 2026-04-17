#!/usr/bin/env python3
# Requirements:
#   pip install datasets nltk english-words

from __future__ import annotations

import argparse
import json
import re
from collections import Counter
from pathlib import Path
from typing import Iterable

from datasets import load_dataset
from datasets.exceptions import DatasetNotFoundError

WORD_RE = re.compile(r"[a-z]+")
URL_RE = re.compile(r"https?://\S+|www\.\S+")
SPACES_RE = re.compile(r"\s+")
NON_CONTEXT_RE = re.compile(r"[^a-z0-9\s\.\,\!\?'\-]")

DEFAULT_MAX_CONTEXTS = 5_000
RTP_TOXICITY_THRESHOLD = 0.8
DEFAULT_MIN_GENERIC_TOXIC_SCORE = 0.5
DEFAULT_MAX_PER_DATASET = 300_000

TOXIC_DATASET_SPECS = [
    {
        "id": "julien-c/jigsaw-toxic-comment-classification-challenge",
        "type": "jigsaw",
        "splits": ["train"],
    },
    {
        "id": "thesofakillers/jigsaw-toxic-comment-classification-challenge",
        "type": "jigsaw",
        "splits": ["train"],
    },
    {
        "id": "tcapelle/jigsaw-toxic-comment-classification-challenge",
        "type": "jigsaw",
        "splits": ["train"],
    },
    {
        "id": "Mirenda/jigsaw-toxic-comment-classification-challenge",
        "type": "jigsaw",
        "splits": ["train"],
    },
    {
        "id": "allenai/real-toxicity-prompts",
        "type": "real_toxicity",
        "splits": ["train"],
    },
    {
        "id": "Ahren09/RealToxicityPrompts",
        "type": "real_toxicity",
        "splits": ["train", "validation", "test"],
    },
    {
        "id": "google/civil_comments",
        "type": "civil_comments",
        "splits": ["train"],
    },
    {
        "id": "tdavidson/hate_speech_offensive",
        "type": "tdavidson",
        "splits": ["train"],
    },
    {
        "id": "SetFit/hate_speech_offensive",
        "type": "tdavidson_setfit",
        "splits": ["train", "test"],
    },
    {
        "id": "SetFit/hate_speech18",
        "type": "binary_label_setfit",
        "splits": ["train", "test"],
    },
    {
        "id": "tweets-hate-speech-detection/tweets_hate_speech_detection",
        "type": "binary_label_tweets",
        "splits": ["train"],
    },
    {
        "id": "ucberkeley-dlab/measuring-hate-speech",
        "type": "berkeley_hate_score",
        "splits": ["train"],
    },
]

TOXIC_STEM_PATTERNS = (
    re.compile(r"f+u+c+k+"),
    re.compile(r"s+h+i+t+"),
    re.compile(r"b+i+t+c+h+"),
    re.compile(r"c+u+n+t+"),
    re.compile(r"n+i+g+g+"),
    re.compile(r"f+a+g+"),
    re.compile(r"w+h+o+r+e+"),
    re.compile(r"s+l+u+t+"),
    re.compile(r"a+s+s+h+o+l+e+"),
    re.compile(r"d+i+c+k+"),
    re.compile(r"c+o+c+k+"),
    re.compile(r"b+a+s+t+a+r+d+"),
)

TOXIC_PHRASE_KEYWORDS = (
    "kill yourself",
    "kys",
    "go die",
    "i will kill",
    "rape",
    "suicide",
    "gas the",
    "nazi",
    "terrorist",
    "scam",
    "fraud",
    "phishing",
)


def to_float(value, default: float = 0.0) -> float:
    try:
        return float(value)
    except (TypeError, ValueError):
        return default


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(
        description="Mine toxic contexts with streaming datasets and generate clean whitelist."
    )
    parser.add_argument(
        "--max-contexts",
        type=int,
        default=DEFAULT_MAX_CONTEXTS,
        help=f"How many high-quality toxic contexts to collect (default: {DEFAULT_MAX_CONTEXTS}).",
    )
    parser.add_argument(
        "--progress-every",
        type=int,
        default=250,
        help="Print progress after this many saved contexts (default: 250).",
    )
    parser.add_argument(
        "--max-per-dataset",
        type=int,
        default=DEFAULT_MAX_PER_DATASET,
        help=f"Max accepted contexts per dataset source (default: {DEFAULT_MAX_PER_DATASET}).",
    )
    parser.add_argument(
        "--min-generic-toxic-score",
        type=float,
        default=DEFAULT_MIN_GENERIC_TOXIC_SCORE,
        help=(
            "Minimum toxicity score for generic multi-source filtering "
            f"(default: {DEFAULT_MIN_GENERIC_TOXIC_SCORE})."
        ),
    )
    parser.add_argument(
        "--enable-lmsys-triplets",
        action="store_true",
        help=(
            "Also mine anchor/negative triplets from LMSYS chat data "
            "(requires access to lmsys/lmsys-chat-1m)."
        ),
    )
    parser.add_argument(
        "--lmsys-dataset",
        default="lmsys/lmsys-chat-1m",
        help="HF dataset id for triplet mining (default: lmsys/lmsys-chat-1m).",
    )
    parser.add_argument(
        "--lmsys-split",
        default="train",
        help="Dataset split for LMSYS triplets (default: train).",
    )
    parser.add_argument(
        "--lmsys-max-triplets",
        type=int,
        default=50_000,
        help="Maximum LMSYS triplets to save (default: 50000).",
    )
    parser.add_argument(
        "--lmsys-max-rows",
        type=int,
        default=1_000_000,
        help="Maximum LMSYS rows to scan (default: 1000000).",
    )
    parser.add_argument(
        "--lmsys-progress-every",
        type=int,
        default=500,
        help="Print LMSYS progress after this many saved triplets (default: 500).",
    )
    parser.add_argument(
        "--lmsys-language-prefix",
        default="en",
        help=(
            "Keep only rows whose language starts with this prefix "
            "(default: en, set empty string to disable)."
        ),
    )
    parser.add_argument(
        "--triplets-output",
        default=str(Path("datasets") / "mined_toxic_triplets.jsonl"),
        help="Output path for mined triplets JSONL.",
    )
    return parser.parse_args()


def normalize_word(token: str) -> str:
    token = token.lower().strip()
    token = re.sub(r"[^a-z]", "", token)
    return token


def tokenize_words(text: str) -> list[str]:
    return [normalize_word(m.group(0)) for m in WORD_RE.finditer(text.lower())]


def clean_context(text: str) -> str:
    text = text.lower()
    text = URL_RE.sub(" ", text)
    text = NON_CONTEXT_RE.sub(" ", text)
    text = SPACES_RE.sub(" ", text).strip()
    return text


def load_nltk_words() -> set[str]:
    print("[whitelist] Loading NLTK words corpus...")
    import nltk
    from nltk.corpus import words as nltk_words

    try:
        raw_words = nltk_words.words()
    except LookupError:
        print("[whitelist] NLTK corpus not found, downloading 'words'...")
        nltk.download("words", quiet=True)
        raw_words = nltk_words.words()

    cleaned = {normalize_word(word) for word in raw_words}
    return {word for word in cleaned if len(word) >= 2}


def load_english_words_package() -> set[str]:
    print("[whitelist] Loading english-words package dictionary...")
    try:
        from english_words import get_english_words_set

        raw_words = get_english_words_set(["web2"], lower=True)
        cleaned = {normalize_word(word) for word in raw_words}
        return {word for word in cleaned if len(word) >= 2}
    except Exception:
        return set()


def generate_whitelist_words() -> set[str]:
    whitelist = set()
    nltk_words = load_nltk_words()
    whitelist.update(nltk_words)
    print(f"[whitelist] NLTK words added: {len(nltk_words):,}")

    package_words = load_english_words_package()
    whitelist.update(package_words)
    print(f"[whitelist] english-words added: {len(package_words):,}")
    print(f"[whitelist] Total unique clean words: {len(whitelist):,}")

    if len(whitelist) < 200_000:
        raise RuntimeError(
            f"Whitelist too small ({len(whitelist):,}). Need > 200,000 clean English words."
        )
    return whitelist


def save_whitelist(whitelist_words: set[str], output_path: Path) -> None:
    output_path.parent.mkdir(parents=True, exist_ok=True)
    with output_path.open("w", encoding="utf-8") as f:
        for word in sorted(whitelist_words):
            f.write(f"{word}\n")
    print(f"[whitelist] Saved {len(whitelist_words):,} words -> {output_path}")


def load_seed_profane_words(repo_root: Path) -> set[str]:
    db_candidates = [
        repo_root / "profanity-destroyer" / "src" / "database" / "moderation-db.json",
        repo_root / "vector_db_engine" / "src" / "embedded_js" / "moderation-db.json",
    ]
    seed = set()

    for path in db_candidates:
        if not path.exists():
            continue
        print(f"[toxicity] Loading profane seed words from {path}")
        with path.open("r", encoding="utf-8-sig") as f:
            data = json.load(f)
        entries = data.get("entries", [])
        if not isinstance(entries, list):
            continue

        for entry in entries:
            if not isinstance(entry, dict):
                continue
            for field_name in ("match", "word"):
                raw_value = entry.get(field_name)
                if isinstance(raw_value, str):
                    parts = raw_value.split("|")
                elif isinstance(raw_value, list):
                    parts = [str(x) for x in raw_value]
                else:
                    parts = []
                for part in parts:
                    for token in tokenize_words(part):
                        if len(token) >= 3:
                            seed.add(token)

    if not seed:
        seed.update(
            {
                "fuck",
                "fucking",
                "shit",
                "bitch",
                "cunt",
                "nigga",
                "nigger",
                "faggot",
                "whore",
                "slut",
                "asshole",
                "dick",
                "cock",
            }
        )
    print(f"[toxicity] Seed profane words: {len(seed):,}")
    return seed


def is_toxic_jigsaw(row: dict) -> bool:
    return any(
        (
            to_float(row.get("toxic", 0)) >= 0.5,
            to_float(row.get("severe_toxic", 0)) >= 0.5,
            to_float(row.get("obscene", 0)) >= 0.5,
            to_float(row.get("insult", 0)) >= 0.5,
            to_float(row.get("identity_hate", 0)) >= 0.5,
        )
    )


def row_text_value(row: dict, candidates: Iterable[str]) -> str:
    for key in candidates:
        value = row.get(key)
        if value is None and "." in key:
            value = nested_get(row, key)
        if isinstance(value, str) and value.strip():
            return value
    return ""


def open_streaming_dataset(spec: dict):
    dataset_id = spec["id"]
    split_candidates = spec.get("splits", ["train", "validation", "test"])
    config_name = spec.get("config")
    last_error = None
    for split in split_candidates:
        try:
            print(f"[datasets] Trying dataset: {dataset_id} ({split})")
            stream = load_dataset(
                dataset_id,
                name=config_name,
                split=split,
                streaming=True,
            )
            print(f"[datasets] Using dataset: {dataset_id} ({split})")
            return split, stream
        except DatasetNotFoundError as exc:
            print(f"[datasets] Not found: {dataset_id}")
            last_error = exc
        except Exception as exc:  # network/permission/runtime issues
            print(f"[datasets] Failed {dataset_id} ({split}): {exc}")
            last_error = exc

    raise RuntimeError(f"Unable to load dataset source: {dataset_id}") from last_error


def nested_get(data: dict, path: str):
    current = data
    for key in path.split("."):
        if not isinstance(current, dict):
            return None
        current = current.get(key)
    return current


def row_toxicity_score_realtoxicity(row: dict) -> float:
    candidates = (
        "toxicity",
        "profanity",
        "severe_toxicity",
        "prompt_toxicity",
        "continuation_toxicity",
        "prompt.toxicity",
        "prompt.profanity",
        "prompt.severe_toxicity",
        "continuation.toxicity",
        "continuation.profanity",
        "continuation.severe_toxicity",
    )
    scores = []
    for path in candidates:
        value = nested_get(row, path)
        try:
            if value is not None:
                scores.append(float(value))
        except (TypeError, ValueError):
            continue
    return max(scores) if scores else 0.0


def to_int(value) -> int | None:
    try:
        if value is None:
            return None
        return int(float(value))
    except Exception:
        return None


def label_text_toxicity(label_text: str) -> bool | None:
    if not label_text:
        return None
    text = label_text.strip().lower()
    if any(
        marker in text
        for marker in ("neither", "not hate", "not_hate", "non-toxic", "clean")
    ):
        return False
    if any(
        marker in text
        for marker in ("hate", "offensive", "toxic", "abusive", "insult")
    ):
        return True
    if text in {"0", "1"}:
        return text == "1"
    return None


def extract_toxic_text_and_score(
    row: dict, spec: dict, min_generic_toxic_score: float
) -> tuple[str, float] | None:
    source_type = spec.get("type", "")

    if source_type == "jigsaw":
        if not is_toxic_jigsaw(row):
            return None
        return (
            row_text_value(row, ("comment_text", "text", "comment", "content")),
            max(
                to_float(row.get("toxic")),
                to_float(row.get("severe_toxic")),
                to_float(row.get("obscene")),
                to_float(row.get("threat")),
                to_float(row.get("insult")),
                to_float(row.get("identity_hate")),
                0.0,
            ),
        )

    if source_type == "real_toxicity":
        score = row_toxicity_score_realtoxicity(row)
        if score < RTP_TOXICITY_THRESHOLD:
            return None
        return (
            row_text_value(
                row,
                (
                    "prompt.text",
                    "continuation.text",
                    "text",
                    "comment_text",
                    "comment",
                    "content",
                ),
            ),
            score,
        )

    if source_type == "civil_comments":
        score = max(
            to_float(row.get("toxicity")),
            to_float(row.get("severe_toxicity")),
            to_float(row.get("obscene")),
            to_float(row.get("threat")),
            to_float(row.get("insult")),
            to_float(row.get("identity_attack")),
            0.0,
        )
        if score < min_generic_toxic_score:
            return None
        return (
            row_text_value(row, ("text", "comment_text", "comment", "content")),
            score,
        )

    if source_type == "tdavidson":
        # 0=hate speech, 1=offensive language, 2=neither
        label = to_int(row.get("class"))
        if label is None:
            label = to_int(row.get("label"))
        if label is None or label == 2:
            return None
        score = 1.0 if label == 0 else 0.95
        return row_text_value(row, ("tweet", "text", "comment", "content")), score

    if source_type == "tdavidson_setfit":
        label_decision = label_text_toxicity(str(row.get("label_text", "")))
        if label_decision is None:
            label = to_int(row.get("label"))
            if label is None:
                return None
            # SetFit conversion of Davidson: 0/1 toxic, 2 clean
            label_decision = label in (0, 1)
        if not label_decision:
            return None
        return row_text_value(row, ("text", "tweet", "comment", "content")), 0.95

    if source_type == "binary_label_setfit":
        label_decision = label_text_toxicity(str(row.get("label_text", "")))
        if label_decision is None:
            label = to_int(row.get("label"))
            if label is None:
                return None
            label_decision = label == 1
        if not label_decision:
            return None
        return row_text_value(row, ("text", "tweet", "comment", "content")), 0.95

    if source_type == "binary_label_tweets":
        label = to_int(row.get("label"))
        if label is None or label == 0:
            return None
        return row_text_value(row, ("tweet", "text", "comment", "content")), 0.95

    if source_type == "berkeley_hate_score":
        score = max(
            to_float(row.get("hate_speech_score")),
            to_float(row.get("hatespeech")),
            to_float(row.get("insult")),
            to_float(row.get("humiliate")),
            to_float(row.get("dehumanize")),
            to_float(row.get("violence")),
            to_float(row.get("genocide")),
            0.0,
        )
        if score < min_generic_toxic_score:
            return None
        return (
            row_text_value(row, ("text", "comment_text", "comment", "content")),
            score,
        )

    return None


def realtoxicity_text(row: dict) -> str:
    continuation = nested_get(row, "continuation.text")
    prompt = nested_get(row, "prompt.text")
    if isinstance(continuation, str) and continuation.strip():
        return continuation
    if isinstance(prompt, str):
        return prompt
    return row_text_value(row, ("text", "prompt", "continuation"))


def extract_profane_terms(
    text: str, seed_profane_words: set[str], whitelist_words: set[str]
) -> list[str]:
    tokens = [tok for tok in tokenize_words(text) if len(tok) >= 3]
    token_set = set(tokens)

    matches = {token for token in token_set if token in seed_profane_words}
    if not matches:
        for token in token_set:
            if token in whitelist_words:
                continue
            if any(pattern.search(token) for pattern in TOXIC_STEM_PATTERNS):
                matches.add(token)

    return sorted(matches)


def contains_toxic_phrase(text: str) -> bool:
    lowered = str(text).lower()
    return any(marker in lowered for marker in TOXIC_PHRASE_KEYWORDS)


def normalize_role(role) -> str:
    value = str(role or "").strip().lower()
    if any(marker in value for marker in ("user", "human", "prompter")):
        return "user"
    if any(marker in value for marker in ("assistant", "bot", "model", "gpt")):
        return "assistant"
    return value


def extract_message_text(message) -> str:
    if isinstance(message, str):
        return message
    if not isinstance(message, dict):
        return ""

    for key in ("content", "text", "value", "message"):
        value = message.get(key)
        if isinstance(value, str) and value.strip():
            return value
        if isinstance(value, list):
            parts = []
            for part in value:
                if isinstance(part, str):
                    parts.append(part)
                elif isinstance(part, dict):
                    text = part.get("text") or part.get("content") or part.get("value")
                    if isinstance(text, str):
                        parts.append(text)
            merged = " ".join(parts).strip()
            if merged:
                return merged
    return ""


def parse_messages_from_container(container) -> list[tuple[str, str]]:
    if isinstance(container, str):
        try:
            container = json.loads(container)
        except Exception:
            return []

    if isinstance(container, dict):
        for key in ("conversation", "conversations", "messages", "turns"):
            nested = container.get(key)
            parsed = parse_messages_from_container(nested)
            if parsed:
                return parsed
        return []

    if not isinstance(container, list):
        return []

    parsed_messages: list[tuple[str, str]] = []
    for item in container:
        if isinstance(item, str):
            text = item.strip()
            if text:
                parsed_messages.append(("", text))
            continue

        if not isinstance(item, dict):
            continue

        role = normalize_role(
            item.get("role")
            or item.get("speaker")
            or item.get("from")
            or item.get("author")
            or ""
        )
        text = extract_message_text(item).strip()
        if text:
            parsed_messages.append((role, text))

    return parsed_messages


def parse_lmsys_messages(row: dict) -> list[tuple[str, str]]:
    for key in (
        "conversation",
        "conversations",
        "messages",
        "turns",
        "chat",
        "conversation_a",
    ):
        parsed = parse_messages_from_container(row.get(key))
        if parsed:
            return parsed
    return []


def extract_lmsys_toxic_terms(
    text: str, seed_profane_words: set[str], whitelist_words: set[str]
) -> list[str]:
    cleaned = clean_context(text)
    profane_terms = extract_profane_terms(cleaned, seed_profane_words, whitelist_words)
    if profane_terms:
        return profane_terms
    if not contains_toxic_phrase(cleaned):
        return []
    fallback_terms = {
        tok
        for tok in tokenize_words(cleaned)
        if len(tok) >= 3 and tok not in whitelist_words
    }
    if fallback_terms:
        return sorted(fallback_terms)
    return ["toxic_phrase"]


def mine_lmsys_triplets(
    dataset_id: str,
    split: str,
    output_path: Path,
    seed_profane_words: set[str],
    whitelist_words: set[str],
    max_triplets: int,
    max_rows: int,
    progress_every: int,
    language_prefix: str,
) -> tuple[int, Counter]:
    print(f"[triplets] Streaming dataset: {dataset_id} ({split})")
    try:
        stream = load_dataset(dataset_id, split=split, streaming=True)
    except Exception as exc:
        print(f"[triplets] Skipping {dataset_id}: {exc}")
        return 0, Counter()

    output_path.parent.mkdir(parents=True, exist_ok=True)
    source_counter: Counter = Counter()
    seen_pairs: set[str] = set()
    scanned_rows = 0
    saved_triplets = 0
    normalized_lang_prefix = language_prefix.strip().lower()
    source_key = f"lmsys_triplets::{dataset_id}@{split}"

    with output_path.open("w", encoding="utf-8") as out:
        for row in stream:
            if scanned_rows >= max_rows or saved_triplets >= max_triplets:
                break
            scanned_rows += 1

            if scanned_rows % 50_000 == 0:
                print(
                    f"[triplets] scanned={scanned_rows:,}, "
                    f"saved={saved_triplets:,}, path={output_path}"
                )

            if not isinstance(row, dict):
                continue

            if normalized_lang_prefix:
                language = str(row.get("language") or row.get("lang") or "").lower()
                if language and not language.startswith(normalized_lang_prefix):
                    continue

            messages = parse_lmsys_messages(row)
            if len(messages) < 2:
                continue

            previous_clean_user = ""
            for idx, (role, text) in enumerate(messages):
                if saved_triplets >= max_triplets:
                    break
                if normalize_role(role) != "user":
                    continue

                cleaned_user = clean_context(text)
                if len(cleaned_user) < 12:
                    continue

                toxic_terms = extract_lmsys_toxic_terms(
                    cleaned_user, seed_profane_words, whitelist_words
                )
                if not toxic_terms:
                    previous_clean_user = cleaned_user
                    continue

                next_assistant = ""
                for next_role, next_text in messages[idx + 1 :]:
                    role_kind = normalize_role(next_role)
                    if role_kind == "assistant":
                        next_assistant = clean_context(next_text)
                        break
                    if role_kind == "user":
                        break

                if len(next_assistant) < 12:
                    continue

                pair_key = f"{cleaned_user}||{next_assistant}"
                if pair_key in seen_pairs:
                    continue
                seen_pairs.add(pair_key)

                triplet = {
                    "source": source_key,
                    "anchor_toxic": cleaned_user,
                    "anchor_terms": toxic_terms,
                    "negative_assistant": next_assistant,
                    "negative_prev_user_clean": previous_clean_user,
                    "language": str(row.get("language") or row.get("lang") or ""),
                }
                out.write(json.dumps(triplet, ensure_ascii=False) + "\n")
                source_counter[source_key] += 1
                saved_triplets += 1

                if saved_triplets % progress_every == 0:
                    print(
                        f"[triplets] Saved {saved_triplets:,}/{max_triplets:,} triplets..."
                    )

    print(
        f"[triplets] Final triplets: {saved_triplets:,} "
        f"(scanned rows: {scanned_rows:,}) -> {output_path}"
    )
    return saved_triplets, source_counter


def mine_toxic_contexts(
    whitelist_words: set[str],
    seed_profane_words: set[str],
    max_contexts: int,
    progress_every: int,
    max_per_dataset: int,
    min_generic_toxic_score: float,
) -> tuple[list[dict], Counter]:
    records: list[dict] = []
    source_counter: Counter = Counter()
    seen_contexts: set[str] = set()

    def try_add_record(source: str, raw_text: str, toxicity_score: float) -> None:
        if len(records) >= max_contexts:
            return

        cleaned = clean_context(raw_text)
        if len(cleaned) < 12 or cleaned in seen_contexts:
            return

        profane_terms = extract_profane_terms(cleaned, seed_profane_words, whitelist_words)
        if not profane_terms:
            return

        seen_contexts.add(cleaned)
        source_counter[source] += 1
        records.append(
            {
                "source": source,
                "toxicity_score": round(float(toxicity_score), 4),
                "profane_terms": profane_terms,
                "context": cleaned,
            }
        )

        if len(records) % progress_every == 0:
            print(f"[toxicity] Saved {len(records):,}/{max_contexts:,} contexts...")

    print("[toxicity] Streaming multi-source toxic datasets...")
    for spec in TOXIC_DATASET_SPECS:
        if len(records) >= max_contexts:
            break

        dataset_id = spec["id"]
        try:
            selected_split, stream = open_streaming_dataset(spec)
        except Exception as exc:
            print(f"[datasets] Skipping {dataset_id}: {exc}")
            continue

        source_key = f"{spec.get('type', 'generic')}::{dataset_id}@{selected_split}"
        scanned = 0
        accepted = 0

        for row in stream:
            if len(records) >= max_contexts or accepted >= max_per_dataset:
                break
            if not isinstance(row, dict):
                continue

            scanned += 1
            if scanned % 50_000 == 0:
                print(
                    f"[toxicity][{dataset_id}] scanned: {scanned:,}, "
                    f"accepted-from-source: {accepted:,}, total-saved: {len(records):,}"
                )

            extracted = extract_toxic_text_and_score(
                row=row,
                spec=spec,
                min_generic_toxic_score=min_generic_toxic_score,
            )
            if not extracted:
                continue

            raw_text, score = extracted
            before = len(records)
            try_add_record(source_key, raw_text, score)
            if len(records) > before:
                accepted += 1

        print(
            f"[toxicity][{dataset_id}] done scanned={scanned:,}, "
            f"accepted={accepted:,}, total-saved={len(records):,}"
        )

    print(f"[toxicity] Final contexts mined: {len(records):,}")
    return records, source_counter


def save_toxic_contexts(records: list[dict], source_counter: Counter, output_path: Path) -> None:
    output_path.parent.mkdir(parents=True, exist_ok=True)
    payload = {
        "total_contexts": len(records),
        "sources": dict(source_counter),
        "records": records,
    }
    with output_path.open("w", encoding="utf-8") as f:
        json.dump(payload, f, ensure_ascii=False, indent=2)
    print(f"[toxicity] Saved {len(records):,} contexts -> {output_path}")


def main() -> None:
    args = parse_args()
    repo_root = Path(__file__).resolve().parents[2]

    whitelist_path = (
        repo_root / "profanity-destroyer" / "src" / "database" / "whitelist.txt"
    )
    mined_toxicity_path = repo_root / "datasets" / "mined_toxicity.json"
    triplets_output_path = Path(args.triplets_output)
    if not triplets_output_path.is_absolute():
        triplets_output_path = (repo_root / triplets_output_path).resolve()

    print("[start] Generating whitelist...")
    whitelist_words = generate_whitelist_words()
    save_whitelist(whitelist_words, whitelist_path)

    print("[start] Mining toxic contexts with streaming datasets...")
    seed_profane_words = load_seed_profane_words(repo_root)
    records, source_counter = mine_toxic_contexts(
        whitelist_words=whitelist_words,
        seed_profane_words=seed_profane_words,
        max_contexts=max(1, args.max_contexts),
        progress_every=max(1, args.progress_every),
        max_per_dataset=max(1, args.max_per_dataset),
        min_generic_toxic_score=max(0.0, min(1.0, args.min_generic_toxic_score)),
    )
    save_toxic_contexts(records, source_counter, mined_toxicity_path)

    if args.enable_lmsys_triplets:
        print("[start] Mining LMSYS toxic-vs-clean triplets...")
        mine_lmsys_triplets(
            dataset_id=str(args.lmsys_dataset).strip(),
            split=str(args.lmsys_split).strip() or "train",
            output_path=triplets_output_path,
            seed_profane_words=seed_profane_words,
            whitelist_words=whitelist_words,
            max_triplets=max(1, args.lmsys_max_triplets),
            max_rows=max(1, args.lmsys_max_rows),
            progress_every=max(1, args.lmsys_progress_every),
            language_prefix=str(args.lmsys_language_prefix),
        )
    print("[done] Mining pipeline completed successfully.")


if __name__ == "__main__":
    main()
