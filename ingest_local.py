import pandas as pd
import time
from sentence_transformers import SentenceTransformer
from qdrant_client import QdrantClient
from qdrant_client.models import PointStruct

print("🔌 Подключение к Qdrant...")
client = QdrantClient("http://localhost:6333")
collection_name = "scam_patterns"

print("🧠 Загрузка модели MiniLM...")
model = SentenceTransformer('all-MiniLM-L6-v2')

print("📥 Чтение локальных датасетов...")
# 1. Discord phishing
df_csv = pd.read_csv(r"C:\Users\PC\Downloads\discord-phishing-scam-detection.csv", encoding='utf-8', encoding_errors='ignore')
discord_scams = df_csv[df_csv['label'] == 1]['msg_content'].dropna().astype(str).tolist()

# 2. Testing.xlsx
df_xlsx = pd.read_excel(r"C:\Users\PC\Downloads\testing.xlsx")
local_scams = df_xlsx[df_xlsx['Type'] == 1]['Text'].dropna().astype(str).tolist()

all_scams = discord_scams + local_scams
print(f"🎯 Найдено {len(discord_scams)} (Discord) + {len(local_scams)} (Excel) = {len(all_scams)} локальных скам-паттернов.")

# To prevent ID collision with the previous HuggingFace bulk (which used IDs starting from 0)
# we fetch the current collection info or simply use UUIDs.
import uuid

batch_size = 200
start_time = time.time()

for i in range(0, len(all_scams), batch_size):
    batch_texts = all_scams[i:i+batch_size]
    
    embeddings = model.encode(batch_texts)
    
    points = [
        PointStruct(
            id=str(uuid.uuid4()),
            vector=embedding.tolist(),
            payload={"text": text, "category": "local_custom_scam"} 
        )
        for text, embedding in zip(batch_texts, embeddings)
    ]
    
    client.upsert(
        collection_name=collection_name,
        points=points
    )
    print(f"✅ Залито {min(i + batch_size, len(all_scams))} / {len(all_scams)}...")

print(f"🚀 ДОПОЛНИТЕЛЬНОЕ ОБУЧЕНИЕ ГОТОВО за {round(time.time() - start_time, 2)} сек.")
