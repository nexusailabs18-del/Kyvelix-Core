from llama_cpp import Llama
import time
import random

print("🧠 Background AI Generation Started\n")

# Load models quietly
tiny = Llama("models/text/tinyllama-1.1b-chat-v1.0.Q4_K_M.gguf", n_ctx=64, n_threads=12, verbose=False)
phi2 = Llama("models/text/phi-2.Q4_K_M.gguf", n_ctx=64, n_threads=12, verbose=False)
mistral = Llama("models/text/mistral-7b-v0.1.Q4_K_M.gguf", n_ctx=64, n_threads=12, verbose=False)

print("✅ TinyLlama + Phi-2 + Mistral loaded!\n")

counts = {"TLS": 0, "UA": 0, "Cookie": 0, "Header": 0, "Behavior": 0, "Geo": 0}
browsers = ["Chrome", "Firefox", "Safari", "Edge", "Opera"]
start_time = time.time()

while True:
    try:
        # Generate all 6 types
        tiny("TLS cipher:", max_tokens=8)
        counts["TLS"] += 1
        
        phi2(f"{random.choice(browsers)} UA:", max_tokens=12)
        counts["UA"] += 1
        
        mistral("Cookie:", max_tokens=8)
        counts["Cookie"] += 1
        
        tiny("Header:", max_tokens=8)
        counts["Header"] += 1
        
        phi2("Behavior:", max_tokens=8)
        counts["Behavior"] += 1
        
        mistral("Geo:", max_tokens=8)
        counts["Geo"] += 1
        
        # Show progress every 5 iterations
        if counts["TLS"] % 5 == 0:
            elapsed = time.time() - start_time
            total = sum(counts.values())
            rate = total / elapsed if elapsed > 0 else 0
            print(f"\r📊 Total: {total} | Rate: {rate:.1f}/s | {counts}", end="")
        
    except KeyboardInterrupt:
        elapsed = time.time() - start_time
        total = sum(counts.values())
        print(f"\n\n✅ FINAL: {total} items in {elapsed:.0f}s ({total/elapsed:.1f}/s)")
        print(counts)
        break
