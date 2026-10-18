from llama_cpp import Llama
import time

model_path = "/home/kumarr/ky-proxy/models/text/tinyllama-1.1b-chat-v1.0.Q4_K_M.gguf"
llm = Llama(model_path, n_ctx=256, n_threads=16, use_mlock=True, verbose=False)

# Measure with DIFFERENT prompts each time (no cache)
prompts = [
    "What is 1+1?",
    "What is 2+2?",
    "What is 3+3?",
    "What is 4+4?",
    "What is 5+5?",
]

best = 0
for p in prompts:
    start = time.time()
    out = llm(p, max_tokens=32)
    elapsed = time.time() - start
    tok_per_sec = 32 / elapsed
    if tok_per_sec > best:
        best = tok_per_sec
    print(f"'{p}' → {tok_per_sec:.1f} tok/s")

print(f"\nBEST: {best:.1f} tok/s")
