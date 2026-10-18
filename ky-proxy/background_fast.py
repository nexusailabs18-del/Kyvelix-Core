from llama_cpp import Llama
import time
import random

print("⚡ FAST Background Generation (TinyLlama Only)\n")

tiny = Llama("models/text/tinyllama-1.1b-chat-v1.0.Q4_K_M.gguf", n_ctx=32, n_threads=12, verbose=False)

print("✅ TinyLlama loaded! Generating at MAX speed...\n")

counts = {"TLS": 0, "UA": 0, "Cookie": 0, "Header": 0, "Behavior": 0, "Geo": 0}
start_time = time.time()

while True:
    try:
        # Generate all 6 types FAST
        tiny("TLS ciphers:", max_tokens=5)
        counts["TLS"] += 1
        
        tiny("User-Agent:", max_tokens=8)
        counts["UA"] += 1
        
        tiny("Cookie:", max_tokens=5)
        counts["Cookie"] += 1
        
        tiny("Header:", max_tokens=5)
        counts["Header"] += 1
        
        tiny("Behavior:", max_tokens=5)
        counts["Behavior"] += 1
        
        tiny("Location:", max_tokens=5)
        counts["Geo"] += 1
        
        # Show progress every 20 iterations
        if counts["TLS"] % 20 == 0:
            elapsed = time.time() - start_time
            total = sum(counts.values())
            rate = total / elapsed if elapsed > 0 else 0
            per_hour = rate * 3600
            print(f"\r📊 {total} items | {rate:.1f}/s | {per_hour:.0f}/hr | {counts}", end="")
        
    except KeyboardInterrupt:
        elapsed = time.time() - start_time
        total = sum(counts.values())
        rate = total / elapsed
        print(f"\n\n✅ Generated {total} items in {elapsed:.0f}s")
        print(f"📊 Rate: {rate:.1f}/sec = {rate*3600:.0f}/hour = {rate*86400:.0f}/day")
        print(counts)
        break
