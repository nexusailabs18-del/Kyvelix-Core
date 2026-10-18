from llama_cpp import Llama
import time
import os

# Force OpenBLAS to use all cores
os.environ["OPENBLAS_NUM_THREADS"] = "16"

model_path = "/home/kumarr/ky-proxy/models/text/tinyllama-1.1b-chat-v1.0.Q4_K_M.gguf"

# Test different configurations
configs = [
    {"threads": 8, "ctx": 512, "label": "Default (8 threads)"},
    {"threads": 16, "ctx": 512, "label": "16 threads"},
    {"threads": 16, "ctx": 256, "label": "16 threads + small ctx"},
    {"threads": 12, "ctx": 256, "label": "12 threads + small ctx"},
]

for cfg in configs:
    try:
        llm = Llama(
            model_path=model_path,
            n_ctx=cfg["ctx"],
            n_threads=cfg["threads"],
            verbose=False
        )
        
        # Warmup
        llm("Hi", max_tokens=4)
        
        # Measure 3 runs, take best
        best = 0
        for _ in range(3):
            start = time.time()
            llm("What is 2+2?", max_tokens=32)
            elapsed = time.time() - start
            tok_per_sec = 32 / elapsed
            if tok_per_sec > best:
                best = tok_per_sec
        
        print(f"{cfg['label']}: {best:.1f} tok/s")
    except Exception as e:
        print(f"{cfg['label']}: Failed - {e}")

# Try with mlock (keep in RAM)
print("\n=== With mlock (RAM locked) ===")
try:
    llm = Llama(
        model_path=model_path,
        n_ctx=256,
        n_threads=16,
        use_mlock=True,
        verbose=False
    )
    llm("Hi", max_tokens=4)
    
    start = time.time()
    llm("What is 2+2?", max_tokens=32)
    elapsed = time.time() - start
    print(f"mlock + 16 threads: {32/elapsed:.1f} tok/s")
except Exception as e:
    print(f"mlock failed: {e}")
