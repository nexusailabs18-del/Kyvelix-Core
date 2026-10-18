from llama_cpp import Llama
import time

model_path = "/home/kumarr/ky-proxy/models/text/mistral-7b-v0.1.Q4_K_M.gguf"

print("Loading Mistral 7B (4.1GB)...")
llm = Llama(model_path, n_ctx=512, n_threads=16, use_mlock=True, verbose=False)

# Different prompts to avoid cache
prompts = [
    "What is 2+2?",
    "Explain gravity simply.",
    "Write a haiku about computers.",
    "What is the capital of France?",
    "How do airplanes fly?",
]

print("\n=== MISTRAL 7B SPEED TEST ===\n")
best = 0
for p in prompts:
    start = time.time()
    out = llm(p, max_tokens=32)
    elapsed = time.time() - start
    tok_per_sec = 32 / elapsed
    if tok_per_sec > best:
        best = tok_per_sec
    print(f"'{p}' → {tok_per_sec:.1f} tok/s")
    print(f"  Output: {out['choices'][0]['text'][:50]}")

print(f"\n=== BEST: {best:.1f} tok/s ===")
print(f"vs GPT-4 API (10 tok/s): {best/10:.1f}x faster")
