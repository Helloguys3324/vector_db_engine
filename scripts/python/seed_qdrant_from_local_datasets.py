import argparse
import os
import time
import uuid
from pathlib import Path

import pandas as pd
from qdrant_client import QdrantClient
from qdrant_client.models import PointStruct
from sentence_transformers import SentenceTransformer


def parse_args():
    parser = argparse.ArgumentParser(
        description="Seed Qdrant from local CSV/XLSX scam datasets."
    )
    parser.add_argument(
        "--qdrant-url",
        default=os.getenv("QDRANT_URL", "http://localhost:6333"),
        help="Qdrant URL (default: env QDRANT_URL or http://localhost:6333).",
    )
    parser.add_argument(
        "--collection",
        default=os.getenv("QDRANT_COLLECTION", "scam_patterns"),
        help="Target collection name (default: env QDRANT_COLLECTION or scam_patterns).",
    )
    parser.add_argument(
        "--model",
        default=os.getenv("EMBEDDING_MODEL", "all-MiniLM-L6-v2"),
        help="SentenceTransformer model (default: env EMBEDDING_MODEL or all-MiniLM-L6-v2).",
    )
    parser.add_argument(
        "--batch-size",
        type=int,
        default=int(os.getenv("SEED_BATCH_SIZE", "200")),
        help="Upsert batch size (default: env SEED_BATCH_SIZE or 200).",
    )
    parser.add_argument(
        "--csv",
        default=os.getenv("LOCAL_SCAM_CSV_PATH"),
        help="Path to discord CSV dataset (or env LOCAL_SCAM_CSV_PATH).",
    )
    parser.add_argument(
        "--xlsx",
        default=os.getenv("LOCAL_SCAM_XLSX_PATH"),
        help="Path to Excel dataset (or env LOCAL_SCAM_XLSX_PATH).",
    )
    return parser.parse_args()


def resolve_input_path(raw_value: str | None, candidates: list[Path], label: str) -> Path:
    if raw_value:
        path = Path(raw_value).expanduser().resolve()
        if path.exists():
            return path
        raise FileNotFoundError(f"{label} file not found: {path}")

    for candidate in candidates:
        if candidate.exists():
            return candidate.resolve()

    hint = "\n".join(f"  - {candidate}" for candidate in candidates)
    raise FileNotFoundError(
        f"{label} file not found. Provide --{label.lower()} or environment variable. Checked:\n{hint}"
    )


def main():
    args = parse_args()
    repo_root = Path(__file__).resolve().parents[2]

    csv_path = resolve_input_path(
        args.csv,
        [
            Path.cwd() / "discord-phishing-scam-detection.csv",
            repo_root / "datasets" / "discord-phishing-scam-detection.csv",
            repo_root / "discord-phishing-scam-detection.csv",
        ],
        "CSV",
    )
    xlsx_path = resolve_input_path(
        args.xlsx,
        [
            Path.cwd() / "testing.xlsx",
            repo_root / "datasets" / "testing.xlsx",
            repo_root / "testing.xlsx",
        ],
        "XLSX",
    )

    print("Connecting to Qdrant...")
    client = QdrantClient(args.qdrant_url)
    collection_name = args.collection

    print("Loading sentence model...")
    model = SentenceTransformer(args.model)

    print("Reading local datasets...")
    df_csv = pd.read_csv(csv_path, encoding="utf-8", encoding_errors="ignore")
    discord_scams = (
        df_csv[df_csv["label"] == 1]["msg_content"].dropna().astype(str).tolist()
    )

    df_xlsx = pd.read_excel(xlsx_path)
    local_scams = df_xlsx[df_xlsx["Type"] == 1]["Text"].dropna().astype(str).tolist()

    all_scams = discord_scams + local_scams
    print(
        f"Found {len(discord_scams)} (CSV) + {len(local_scams)} (XLSX) = {len(all_scams)} local scam patterns."
    )

    batch_size = max(1, args.batch_size)
    start_time = time.time()

    for i in range(0, len(all_scams), batch_size):
        batch_texts = all_scams[i : i + batch_size]
        embeddings = model.encode(batch_texts)

        points = [
            PointStruct(
                id=str(uuid.uuid4()),
                vector=embedding.tolist(),
                payload={"text": text, "category": "local_custom_scam"},
            )
            for text, embedding in zip(batch_texts, embeddings)
        ]

        client.upsert(collection_name=collection_name, points=points)
        print(f"Uploaded {min(i + batch_size, len(all_scams))} / {len(all_scams)}")

    print(f"Done in {round(time.time() - start_time, 2)}s")


if __name__ == "__main__":
    main()
