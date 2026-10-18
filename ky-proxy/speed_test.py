from llama_cpp import Llama
import time

path = "/home/kumarr/ky-proxy/models/text/tinyllama-1.1b-chat-v1.0.Q4_K_M.gguf"

# Try different thread counts
for threads in [8, 12, 16]:
    try:
        llm = Llama(model_path=path, n_ctx=256, n_threads=threads, verbose=False)
        llm("Hi", max_tokens=4)  # warmup
        
        start = time.time()
        llm("Count to 10", max_tokens=32)
        elapsed = time.time() - start
        
        print(f"Threads: {threads:2d} → {32/elapsed:.1f} tok/s")
    except Exception as e:
        print(f"Threads: {threads:2d} → Failed: {e}")
