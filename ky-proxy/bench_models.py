import time
from llama_cpp import Llama
import os

# Models are in ky-proxy/models/text/
base = "/home/kumarr/ky-proxy/models/text"

models = [
    ("TinyLlama 1.1B", f"{base}/tinyllama-1.1b-chat-v1.0.Q4_K_M.gguf"),
    ("Phi-2 2.7B", f"{base}/phi-2.Q4_K_M.gguf"),
    ("Mistral 7B", f"{base}/mistral-7b-v0.1.Q4_K_M.gguf"),
]

for name, path in models:
    print(f"\n=== {name} ===")
    print(f"  Path: {path}")
    print(f"  Exists: {os.path.exists(path)}")
    
    if not os.path.exists(path):
        print("  ❌ Not found — skipping")
        continue
    
    try:
        llm = Llama(model_path=path, n_ctx=512, n_threads=8, verbose=False)
        
        # Warmup
        llm("Hello", max_tokens=8)
        
        # Measure
        start = time.time()
        output = llm("What is 2+2?", max_tokens=32)
        elapsed = time.time() - start
        
        tok_per_sec = 32 / elapsed
        
        print(f"  Time: {elapsed:.2f}s")
        print(f"  Tokens/sec: {tok_per_sec:.1f}")
        print(f"  Output: {output['choices'][0]['text'][:100]}")
        
    except Exception as e:
        print(f"  ❌ Failed: {e}")
