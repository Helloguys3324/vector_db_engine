import time
import uuid
from sentence_transformers import SentenceTransformer
from qdrant_client import QdrantClient
from qdrant_client.models import PointStruct

client = QdrantClient("http://localhost:6333")
model = SentenceTransformer('all-MiniLM-L6-v2')

nitro_scams = [
    "Giving 3 month Nitro for",
    "Giving 3 month Nitro just join this link",
    "Free discord nitro just click",
    "Click here for free nitro discord",
    "Get your free nitro right now",
    "Discord nitro code generator",
    "Free discord nitro for everyone joining this server",
    "dm me for free nitro",
    "nitro giveaway click link"
]

embeddings = model.encode(nitro_scams)

points = [
    PointStruct(
        id=str(uuid.uuid4()),
        vector=embedding.tolist(),
        payload={"text": text, "category": "nitro_scam"} 
    )
    for text, embedding in zip(nitro_scams, embeddings)
]

client.upsert(
    collection_name="scam_patterns",
    points=points
)
print("✅ Успешно: Нитро-скам зашит в ДНК нейросети.")
