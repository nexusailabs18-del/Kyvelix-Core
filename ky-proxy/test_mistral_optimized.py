from llama_cpp import Llama
import time
import os

# Force OpenBLAS to use all cores
os.environ["OPENBLAS_NUM_THREADS"] = "16"
os.environ["OMP_NUM_THREADS"] = "16"

model_path = "/home/kumarr/ky-proxy/models/text/mistral-7b-v0.1.Q4_K_M.gguf"

print("Loading Mistral 7B optimized...")
llm = Llama(
    model_path=model_path,
    n_ctx=256,          # Small context = fits L3 cache
    n_threads=16,       # All cores
    n_batch=512,        # Bigger batch = more parallelism
    use_mlock=True,     # Lock in RAM
    verbose=False
)

prompts = [
    "What is 2+2?",
    "Explain gravity.",
    "Write a haiku.",
]

print("\n=== MISTRAL 7B OPTIMIZED ===\n")
for p in prompts:
    start = time.time()
    out = llm(p, max_tokens=32)
    elapsed = time.time() - start
    tok_per_sec = 32 / elapsed
    print(f"'{p}' → {tok_per_sec:.1f} tok/s")

# Try with GPU offload to iGPU if available
print("\n=== WITH iGPU OFFLOAD ===")
try:
    llm_gpu = Llama(
        model_path=model_path,
        n_ctx=256,
        n_threads=8,
        n_gpu_layers=20,  # Offload 20 layers to iGPU
        verbose=False
    )
    start = time.time()
    out = llm_gpu("What is 2+2?", max_tokens=32)
    elapsed = time.time() - start
    print(f"iGPU: {32/elapsed:.1f} tok/s")
except Exception as e:
    print(f"iGPU failed: {e}")
