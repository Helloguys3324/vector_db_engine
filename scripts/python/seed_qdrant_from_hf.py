import argparse
import os
import time

from datasets import load_dataset
from qdrant_client import QdrantClient
from qdrant_client.models import Distance, PointStruct, VectorParams
from sentence_transformers import SentenceTransformer


def parse_args():
    parser = argparse.ArgumentParser(
        description="Seed Qdrant scam collection from Hugging Face datasets."
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
        "--sms-dataset",
        default=os.getenv("SMS_DATASET", "sms_spam"),
        help="HF dataset id for SMS spam (default: sms_spam).",
    )
    parser.add_argument(
        "--enron-dataset",
        default=os.getenv("ENRON_DATASET", "SetFit/enron_spam"),
        help="HF dataset id for Enron spam (default: SetFit/enron_spam).",
    )
    return parser.parse_args()


def main():
    args = parse_args()

    print("Connecting to Qdrant...")
    client = QdrantClient(args.qdrant_url)
    collection_name = args.collection

    if client.collection_exists(collection_name):
        client.delete_collection(collection_name)

    client.create_collection(
        collection_name=collection_name,
        vectors_config=VectorParams(size=384, distance=Distance.COSINE),
    )

    print("Loading sentence model...")
    model = SentenceTransformer(args.model)

    print("Loading Hugging Face datasets...")
    dataset_sms = load_dataset(args.sms_dataset, split="train")
    dataset_enron = load_dataset(args.enron_dataset, split="train")

    scam_messages = [row["sms"].strip() for row in dataset_sms if row["label"] == 1]
    enron_spam = [row["text"].strip() for row in dataset_enron if row["label"] == 1]
    all_scams = scam_messages + enron_spam

    print(
        f"Found {len(all_scams)} scam messages (SMS + Enron). Starting vectorization..."
    )

    batch_size = max(1, args.batch_size)
    start_time = time.time()

    for i in range(0, len(all_scams), batch_size):
        batch_texts = all_scams[i : i + batch_size]
        embeddings = model.encode(batch_texts)

        points = [
            PointStruct(
                id=i + j,
                vector=embedding.tolist(),
                payload={"text": text, "category": "general_scam"},
            )
            for j, (text, embedding) in enumerate(zip(batch_texts, embeddings))
        ]

        client.upsert(collection_name=collection_name, points=points)
        print(f"Uploaded {i + len(batch_texts)} / {len(all_scams)}")

    print(f"Done in {round(time.time() - start_time, 2)}s")


if __name__ == "__main__":
    main()
