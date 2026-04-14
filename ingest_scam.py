import time
from datasets import load_dataset
from sentence_transformers import SentenceTransformer
from qdrant_client import QdrantClient
from qdrant_client.models import PointStruct, VectorParams, Distance

print("🔌 Подключение к Qdrant...")
client = QdrantClient("http://localhost:6333")
collection_name = "scam_patterns"

if client.collection_exists(collection_name):
    client.delete_collection(collection_name)

client.create_collection(
    collection_name=collection_name,
    vectors_config=VectorParams(size=384, distance=Distance.COSINE),
)

print("🧠 Загрузка модели MiniLM...")
model = SentenceTransformer('all-MiniLM-L6-v2')

print("📥 Скачивание датасетов из Hugging Face...")
dataset_sms = load_dataset("sms_spam", split="train")
dataset_enron = load_dataset("SetFit/enron_spam", split="train")

scam_messages = [row["sms"].strip() for row in dataset_sms if row["label"] == 1]
enron_spam = [row["text"].strip() for row in dataset_enron if row["label"] == 1]

# Mix them
all_scams = scam_messages + enron_spam

print(f"🎯 Найдено {len(all_scams)} чистых паттернов скама (SMS + Enron). Начинаем векторизацию...")

batch_size = 200
start_time = time.time()

for i in range(0, len(all_scams), batch_size):
    batch_texts = all_scams[i:i+batch_size]
    
    embeddings = model.encode(batch_texts)
    
    points = [
        PointStruct(
            id=i + j,
            vector=embedding.tolist(),
            payload={"text": text, "category": "general_scam"} 
        )
        for j, (text, embedding) in enumerate(zip(batch_texts, embeddings))
    ]
    
    client.upsert(
        collection_name=collection_name,
        points=points
    )
    print(f"✅ Загружено {i + len(batch_texts)} / {len(all_scams)}...")

print(f"🚀 ГОТОВО! База обучена за {round(time.time() - start_time, 2)} сек.")
