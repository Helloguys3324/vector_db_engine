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

    out_path = repo_root / "rust_dict.txt"
    with out_path.open("w", encoding="utf-8") as f:
        for word in sorted(words):
            f.write(f"{word}\n")

    print(f"Compiled {len(words)} rules from moderation-db and naughty-words into rust_dict.txt")


if __name__ == "__main__":
    main()
