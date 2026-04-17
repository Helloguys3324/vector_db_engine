import argparse
import hashlib
import json
import os
import time
from pathlib import Path

LIVE_TRAINED_PROFANITY_CATEGORY = "live_trained_profanity"
MINED_CONTEXT_CATEGORY = "mined_toxic_context"


def parse_args():
    parser = argparse.ArgumentParser(
        description="Seed Qdrant from datasets/mined_toxicity.json for profanity L2 matching."
    )
    parser.add_argument(
        "--input",
        default=os.getenv(
            "MINED_TOXICITY_PATH",
            str(Path("datasets") / "mined_toxicity.json"),
        ),
        help="Path to mined_toxicity.json.",
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
        default=int(os.getenv("SEED_BATCH_SIZE", "128")),
        help="Upsert batch size (default: env SEED_BATCH_SIZE or 128).",
    )
    parser.add_argument(
        "--max-records",
        type=int,
        default=None,
        help="Process only first N mined records (default: all).",
    )
    parser.add_argument(
        "--min-term-length",
        type=int,
        default=3,
        help="Minimum profane term length to keep (default: 3).",
    )
    parser.add_argument(
        "--include-contexts",
        action="store_true",
        help="Also index full toxic contexts under category mined_toxic_context.",
    )
    parser.add_argument(
        "--dry-run",
        action="store_true",
        help="Parse and count points without loading model or writing to Qdrant.",
    )
    return parser.parse_args()


def normalize_term(term: str) -> str:
    return "".join(ch for ch in str(term).strip().lower() if ch.isalpha())


def normalize_context(text: str) -> str:
    text = str(text).strip().lower()
    return " ".join(text.split())


def to_float(value, default: float = 0.0) -> float:
    try:
        return float(value)
    except (TypeError, ValueError):
        return default


def deterministic_point_id(namespace: str, text: str) -> str:
    digest = hashlib.sha1(f"{namespace}::{text}".encode("utf-8")).hexdigest()
    return f"{digest[:8]}-{digest[8:12]}-{digest[12:16]}-{digest[16:20]}-{digest[20:32]}"


def load_items(
    input_path: Path,
    min_term_length: int,
    include_contexts: bool,
    max_records: int | None = None,
):
    with input_path.open("r", encoding="utf-8") as f:
        payload = json.load(f)

    records = payload.get("records", [])
    if not isinstance(records, list):
        raise ValueError("Invalid mined_toxicity format: records must be a list")

    term_items = {}
    context_items = {}

    for idx, row in enumerate(records):
        if max_records is not None and idx >= max_records:
            break
        if not isinstance(row, dict):
            continue
        source = str(row.get("source", "unknown"))
        score = to_float(row.get("toxicity_score", 0.0))
        terms = row.get("profane_terms", [])
        context = normalize_context(row.get("context", ""))

        if isinstance(terms, list):
            for raw_term in terms:
                term = normalize_term(raw_term)
                if len(term) < min_term_length:
                    continue
                existing = term_items.get(term)
                if existing is None:
                    term_items[term] = {
                        "text": term,
                        "category": LIVE_TRAINED_PROFANITY_CATEGORY,
                        "source": "mined_toxicity_terms",
                        "dataset_source": source,
                        "toxicity_score": score,
                    }
                else:
                    existing["toxicity_score"] = max(existing["toxicity_score"], score)

        if include_contexts and len(context) >= 16:
            key = context[:512]
            if key not in context_items:
                context_items[key] = {
                    "text": key,
                    "category": MINED_CONTEXT_CATEGORY,
                    "source": "mined_toxicity_contexts",
                    "dataset_source": source,
                    "toxicity_score": score,
                }

    items = list(term_items.values()) + list(context_items.values())
    return records, items, len(term_items), len(context_items)


def ensure_collection(client, collection: str, vector_params_cls, distance_enum):
    if client.collection_exists(collection):
        return
    client.create_collection(
        collection_name=collection,
        vectors_config=vector_params_cls(size=384, distance=distance_enum.COSINE),
    )


def main():
    args = parse_args()
    repo_root = Path(__file__).resolve().parents[2]
    input_path = Path(args.input)
    if not input_path.is_absolute():
        input_path = (repo_root / input_path).resolve()

    if not input_path.exists():
        raise FileNotFoundError(f"Input file not found: {input_path}")

    print(f"Reading mined toxicity from: {input_path}")
    records, items, terms_count, contexts_count = load_items(
        input_path=input_path,
        min_term_length=max(1, args.min_term_length),
        include_contexts=args.include_contexts,
        max_records=args.max_records if args.max_records and args.max_records > 0 else None,
    )

    print(f"Parsed mined records: {len(records):,}")
    print(f"Prepared term points: {terms_count:,}")
    if args.include_contexts:
        print(f"Prepared context points: {contexts_count:,}")
    print(f"Total points to upsert: {len(items):,}")

    if not items:
        print("Nothing to upsert.")
        return

    if args.dry_run:
        print("Dry run completed.")
        return

    try:
        from qdrant_client import QdrantClient
        from qdrant_client.models import Distance, PointStruct, VectorParams
        from sentence_transformers import SentenceTransformer
    except ImportError as exc:
        raise RuntimeError(
            "Missing dependencies for ingestion. Install with: "
            "python -m pip install qdrant-client sentence-transformers"
        ) from exc

    print(f"Connecting to Qdrant: {args.qdrant_url}")
    client = QdrantClient(args.qdrant_url)
    ensure_collection(client, args.collection, VectorParams, Distance)

    print(f"Loading sentence model: {args.model}")
    model = SentenceTransformer(args.model)

    batch_size = max(1, args.batch_size)
    start_time = time.time()

    for i in range(0, len(items), batch_size):
        batch = items[i : i + batch_size]
        texts = [item["text"] for item in batch]
        embeddings = model.encode(texts)

        points = []
        for item, embedding in zip(batch, embeddings):
            point_id = deterministic_point_id(item["category"], item["text"])
            points.append(
                PointStruct(
                    id=point_id,
                    vector=embedding.tolist(),
                    payload=item,
                )
            )

        client.upsert(collection_name=args.collection, points=points)
        print(f"Upserted {min(i + batch_size, len(items)):,} / {len(items):,}")

    print(f"Done in {round(time.time() - start_time, 2)}s")


if __name__ == "__main__":
    main()
