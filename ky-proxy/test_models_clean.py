from llama_cpp import Llama
import time

print("🧪 Testing AI Models\n")

# Test TinyLlama
print("=== TINYLLAMA ===")
tinyllama = Llama(
    model_path="models/text/tinyllama-1.1b-chat-v1.0.Q4_K_M.gguf",
    n_ctx=256,
    n_threads=12,
)

start = time.time()
output = tinyllama(
    "Q: Generate 5 TLS cipher suite IDs for Chrome. A:",
    max_tokens=30,
    temperature=0.1,
    echo=False,
)
print(f"Time: {time.time() - start:.2f}s")
print(f"Output: {output['choices'][0]['text']}")
print("---\n")

# Test Phi-2
print("=== PHI-2 ===")
phi2 = Llama(
    model_path="models/text/phi-2.Q4_K_M.gguf",
    n_ctx=256,
    n_threads=12,
)

start = time.time()
output = phi2(
    "Q: Generate 5 TLS cipher suite IDs for Chrome. A:",
    max_tokens=30,
    temperature=0.1,
    echo=False,
)
print(f"Time: {time.time() - start:.2f}s")
print(f"Output: {output['choices'][0]['text']}")
print("---\n")

print("DONE!")
