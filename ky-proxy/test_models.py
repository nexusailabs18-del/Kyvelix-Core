from llama_cpp import Llama
import time

print("🧪 Testing AI Models for TLS Generation\n")

# Test TinyLlama
print("=== TINYLLAMA TEST ===")
start = time.time()
tinyllama = Llama(
    model_path="models/text/tinyllama-1.1b-chat-v1.0.Q4_K_M.gguf",
    n_ctx=512,
    n_threads=12,
)
print(f"✅ Loaded in {time.time() - start:.2f}s")

start = time.time()
output = tinyllama(
    "Generate 5 TLS cipher suite IDs for Chrome browser",
    max_tokens=50,
    temperature=0.7,
)
print(f"✅ Generated in {time.time() - start:.2f}s")
print(f"Output: {output['choices'][0]['text'][:150]}\n")

# Test Phi-2
print("=== PHI-2 TEST ===")
start = time.time()
phi2 = Llama(
    model_path="models/text/phi-2.Q4_K_M.gguf",
    n_ctx=512,
    n_threads=12,
)
print(f"✅ Loaded in {time.time() - start:.2f}s")

start = time.time()
output = phi2(
    "Generate 5 TLS cipher suite IDs for Chrome browser",
    max_tokens=50,
    temperature=0.7,
)
print(f"✅ Generated in {time.time() - start:.2f}s")
print(f"Output: {output['choices'][0]['text'][:150]}\n")

print("=== COMPARISON COMPLETE ===")
