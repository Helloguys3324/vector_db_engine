import argparse
import os
import uuid

from qdrant_client import QdrantClient
from qdrant_client.models import PointStruct
from sentence_transformers import SentenceTransformer


def parse_args():
    parser = argparse.ArgumentParser(
        description="Seed Qdrant with built-in Discord Nitro scam examples."
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
    return parser.parse_args()


def main():
    args = parse_args()
    client = QdrantClient(args.qdrant_url)
    model = SentenceTransformer(args.model)

    nitro_scams = [
        "Giving 3 month Nitro for",
        "Giving 3 month Nitro just join this link",
        "Free discord nitro just click",
        "Click here for free nitro discord",
        "Get your free nitro right now",
        "Discord nitro code generator",
        "Free discord nitro for everyone joining this server",
        "dm me for free nitro",
        "nitro giveaway click link",
    ]

    embeddings = model.encode(nitro_scams)

    points = [
        PointStruct(
            id=str(uuid.uuid4()),
            vector=embedding.tolist(),
            payload={"text": text, "category": "nitro_scam"},
        )
        for text, embedding in zip(nitro_scams, embeddings)
    ]

    client.upsert(collection_name=args.collection, points=points)
    print("Nitro scam examples uploaded.")


if __name__ == "__main__":
    main()
