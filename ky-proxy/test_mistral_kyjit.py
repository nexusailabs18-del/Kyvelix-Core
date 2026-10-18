import ctypes
import time

# Load dependencies in order
try:
    ctypes.CDLL("libOpenCL.so.1")
    print("✅ OpenCL loaded")
except:
    print("⚠️ OpenCL not available")

try:
    ctypes.CDLL("libopenblas.so.0")
    print("✅ OpenBLAS loaded")
except:
    print("⚠️ OpenBLAS not available")

# Now load KYJIT
try:
    lib = ctypes.CDLL('/home/kumarr/kyjit-enterprise/libkyjit_complete.so')
    print("✅ KYJIT loaded")
except Exception as e:
    print(f"❌ KYJIT failed: {e}")
    print("Trying with RTLD_LAZY...")
    import os
    os.environ["LD_PRELOAD"] = "/usr/lib/x86_64-linux-gnu/libOpenCL.so.1"
    lib = ctypes.CDLL('/home/kumarr/kyjit-enterprise/libkyjit_complete.so', mode=ctypes.RTLD_LAZY)
    print("✅ KYJIT loaded (lazy)")

# Now test Mistral
from llama_cpp import Llama

model_path = "/home/kumarr/ky-proxy/models/text/mistral-7b-v0.1.Q4_K_M.gguf"
print("\nLoading Mistral 7B...")
llm = Llama(model_path, n_ctx=512, n_threads=16, n_batch=1024, use_mlock=True, verbose=False)

prompts = ["What is 2+2?", "Explain gravity.", "Write a haiku."]

print("\n=== MISTRAL 7B SPEED ===\n")
best = 0
for p in prompts:
    start = time.time()
    out = llm(p, max_tokens=32)
    elapsed = time.time() - start
    tok = 32 / elapsed
    best = max(best, tok)
    print(f"'{p}' → {tok:.1f} tok/s")

print(f"\nBEST: {best:.1f} tok/s")
