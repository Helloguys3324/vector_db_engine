import json
import os

words = set()

# 1. Parse NPM naughty-words
npm_path = r"D:\gemini\profanity-destroyer\node_modules\naughty-words\en.json"
if os.path.exists(npm_path):
    with open(npm_path, 'r', encoding='utf-8') as f:
        data = json.load(f)
        for w in data:
            words.add(w.strip().lower())

# 2. Parse custom en.json
en_path = r"D:\gemini\profanity-destroyer\en.json"
if os.path.exists(en_path):
    with open(en_path, 'r', encoding='utf-8') as f:
        data = json.load(f)
        for item in data:
            # check severity if you want, usually old bot did minSeverity = 2
            sev = item.get('severity', 3)
            # if sev < 2: continue
            match_str = item.get('match', '')
            for part in match_str.split('|'):
                clean = part.replace('*', '').strip().lower()
                if len(clean) >= 2:
                    words.add(clean)

# 3. Parse custom blacklist JSON (managed via mini UI)
custom_blacklist_path = r"D:\gemini\profanity-destroyer\src\database\custom-blacklist.json"
if os.path.exists(custom_blacklist_path):
    with open(custom_blacklist_path, 'r', encoding='utf-8') as f:
        raw = f.read()

    parsed = None
    try:
        parsed = json.loads(raw)
    except json.JSONDecodeError:
        parsed = None

    if isinstance(parsed, list):
        for item in parsed:
            if isinstance(item, dict):
                match_str = item.get('match', '')
                if not isinstance(match_str, str):
                    continue
                for part in match_str.split('|'):
                    clean = part.replace('*', '').strip().lower()
                    if len(clean) >= 2:
                        words.add(clean)
            elif isinstance(item, str):
                clean = item.replace('*', '').strip().lower()
                if len(clean) >= 2:
                    words.add(clean)
    else:
        for line in raw.splitlines():
            clean = line.replace('*', '').strip().lower()
            if len(clean) >= 2 and not clean.startswith('#'):
                words.add(clean)

# 4. Save to flat file for Rust Aho-Corasick Engine
out_path = r"D:\gemini\rust_dict.txt"
with open(out_path, 'w', encoding='utf-8') as f:
    for w in sorted(list(words)):
        f.write(w + '\n')

print(f"Compiled {len(words)} rules from en.json, custom-blacklist, and naughty-words into rust_dict.txt")
