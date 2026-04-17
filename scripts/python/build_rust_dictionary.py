import json
from pathlib import Path


def normalize(token: str) -> str:
    return token.replace("*", "").strip().lower()


def add_match_variants(words: set[str], value) -> None:
    if isinstance(value, str):
        for part in value.split("|"):
            clean = normalize(part)
            if len(clean) >= 2:
                words.add(clean)
        return

    if isinstance(value, list):
        for item in value:
            add_match_variants(words, item)


def add_legacy_external_words(words: set[str], path: Path) -> None:
    if not path.exists():
        return

    with path.open("r", encoding="utf-8-sig") as f:
        data = json.load(f)

    if not isinstance(data, list):
        return

    for item in data:
        if not isinstance(item, dict):
            continue
        lang = str(item.get("lang", "en")).strip().lower()
        if lang and lang != "en":
            continue
        severity = item.get("severity", 3)
        try:
            if int(severity) < 2:
                continue
        except (TypeError, ValueError):
            pass

        raw_word = item.get("word")
        if not isinstance(raw_word, str):
            continue
        word = normalize(raw_word)
        if len(word) >= 2:
            words.add(word)


def main() -> None:
    repo_root = Path(__file__).resolve().parents[2]
    words: set[str] = set()

    npm_path = repo_root / "profanity-destroyer" / "node_modules" / "naughty-words" / "en.json"
    if npm_path.exists():
        with npm_path.open("r", encoding="utf-8") as f:
            data = json.load(f)
        if isinstance(data, list):
            for raw_word in data:
                word = normalize(str(raw_word))
                if len(word) >= 2:
                    words.add(word)

    moderation_db_path = repo_root / "profanity-destroyer" / "src" / "database" / "moderation-db.json"
    if moderation_db_path.exists():
        with moderation_db_path.open("r", encoding="utf-8-sig") as f:
            data = json.load(f)

        entries = data.get("entries", [])
        if isinstance(entries, list):
            for item in entries:
                if isinstance(item, dict):
                    add_match_variants(words, item.get("match", ""))
                    add_match_variants(words, item.get("word", ""))
                else:
                    add_match_variants(words, item)

    legacy_external_embedded_path = (
        repo_root / "vector_db_engine" / "src" / "embedded_js" / "merged-external.json"
    )
    add_legacy_external_words(words, legacy_external_embedded_path)

    out_path = repo_root / "rust_dict.txt"
    with out_path.open("w", encoding="utf-8") as f:
        for word in sorted(words):
            f.write(f"{word}\n")

    print(
        f"Compiled {len(words)} rules from moderation-db, naughty-words and legacy-external into rust_dict.txt"
    )


if __name__ == "__main__":
    main()
