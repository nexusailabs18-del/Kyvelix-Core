from llama_cpp import Llama
import time
import json

print("🧠 REAL AI MODEL TEST - Generating Actual Content\n")

# Load all 3 models
print("Loading TinyLlama...")
tiny = Llama("models/text/tinyllama-1.1b-chat-v1.0.Q4_K_M.gguf", n_ctx=128, n_threads=12)

print("Loading Phi-2...")
phi2 = Llama("models/text/phi-2.Q4_K_M.gguf", n_ctx=128, n_threads=12)

print("Loading Mistral 7B...")
mistral = Llama("models/text/mistral-7b-v0.1.Q4_K_M.gguf", n_ctx=128, n_threads=12)

print("\n=== TEST 1: TLS Cipher Generation ===")
print("TinyLlama:")
out = tiny("Generate 5 TLS cipher suite IDs as numbers: 1301, 1302,", max_tokens=20)
print(f"  {out['choices'][0]['text'].strip()}")

print("Phi-2:")
out = phi2("List 5 TLS cipher suites:", max_tokens=20)
print(f"  {out['choices'][0]['text'].strip()}")

print("Mistral:")
out = mistral("Generate TLS cipher order for Chrome:", max_tokens=20)
print(f"  {out['choices'][0]['text'].strip()}")

print("\n=== TEST 2: User-Agent Generation ===")
print("TinyLlama:")
out = tiny("Generate a Chrome User-Agent string:", max_tokens=30)
print(f"  {out['choices'][0]['text'].strip()}")

print("Phi-2:")
out = phi2("Mozilla/5.0 User-Agent for Firefox:", max_tokens=30)
print(f"  {out['choices'][0]['text'].strip()}")

print("Mistral:")
out = mistral("Generate Safari User-Agent:", max_tokens=30)
print(f"  {out['choices'][0]['text'].strip()}")

print("\n=== TEST 3: Cookie Generation ===")
print("TinyLlama:")
out = tiny("Generate a session cookie value:", max_tokens=20)
print(f"  {out['choices'][0]['text'].strip()}")

print("Phi-2:")
out = phi2("session_id=", max_tokens=20)
print(f"  {out['choices'][0]['text'].strip()}")

print("Mistral:")
out = mistral("Generate csrf_token value:", max_tokens=20)
print(f"  {out['choices'][0]['text'].strip()}")

print("\n=== SPEED TEST: How Many Generations Per Minute ===")
start = time.time()
count = 0
for _ in range(10):
    tiny("Generate random value:", max_tokens=10)
    count += 1
elapsed = time.time() - start
print(f"TinyLlama: {count/elapsed:.1f} generations/sec")

start = time.time()
count = 0
for _ in range(5):
    phi2("Generate random value:", max_tokens=10)
    count += 1
elapsed = time.time() - start
print(f"Phi-2: {count/elapsed:.1f} generations/sec")

start = time.time()
count = 0
for _ in range(3):
    mistral("Generate random value:", max_tokens=10)
    count += 1
elapsed = time.time() - start
print(f"Mistral: {count/elapsed:.1f} generations/sec")

print("\n✅ AI Models REALLY working!")
