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

# 3. Save to flat file for Rust Aho-Corasick Engine
out_path = r"D:\gemini\rust_dict.txt"
with open(out_path, 'w', encoding='utf-8') as f:
    for w in sorted(list(words)):
        f.write(w + '\n')

print(f"✅ Успешно скомпилировано {len(words)} правил из en.json и naughty-words в единый rust_dict.txt")
