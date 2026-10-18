from llama_cpp import Llama
import time
import json
import os

print("💾 Background Generation WITH SAVING\n")

tiny = Llama("models/text/tinyllama-1.1b-chat-v1.0.Q4_K_M.gguf", n_ctx=32, n_threads=12, verbose=False)

# Create output directory
os.makedirs("generated", exist_ok=True)

counts = {"TLS": 0, "UA": 0, "Cookie": 0, "Header": 0, "Behavior": 0, "Geo": 0}
data_batch = []
start_time = time.time()

while True:
    try:
        # Generate and capture output
        tls_out = tiny("TLS ciphers:", max_tokens=5)
        tls_text = tls_out['choices'][0]['text'].strip()
        counts["TLS"] += 1
        
        ua_out = tiny("User-Agent:", max_tokens=8)
        ua_text = ua_out['choices'][0]['text'].strip()
        counts["UA"] += 1
        
        cookie_out = tiny("Cookie:", max_tokens=5)
        cookie_text = cookie_out['choices'][0]['text'].strip()
        counts["Cookie"] += 1
        
        header_out = tiny("Header:", max_tokens=5)
        header_text = header_out['choices'][0]['text'].strip()
        counts["Header"] += 1
        
        behavior_out = tiny("Behavior:", max_tokens=5)
        behavior_text = behavior_out['choices'][0]['text'].strip()
        counts["Behavior"] += 1
        
        geo_out = tiny("Location:", max_tokens=5)
        geo_text = geo_out['choices'][0]['text'].strip()
        counts["Geo"] += 1
        
        # Save to batch
        data_batch.append({
            "tls": tls_text,
            "ua": ua_text,
            "cookie": cookie_text,
            "header": header_text,
            "behavior": behavior_text,
            "geo": geo_text,
        })
        
        # Save every 100 items
        if len(data_batch) >= 100:
            with open(f"generated/batch_{int(time.time())}.json", "w") as f:
                json.dump(data_batch, f, indent=2)
            print(f"\n💾 Saved {len(data_batch)} items to generated/")
            data_batch = []
        
        if counts["TLS"] % 20 == 0:
            elapsed = time.time() - start_time
            total = sum(counts.values())
            print(f"\r📊 {total} items | {total/elapsed:.1f}/s | Saved: {counts['TLS']//100} files", end="")
        
    except KeyboardInterrupt:
        # Save remaining
        if data_batch:
            with open(f"generated/final_{int(time.time())}.json", "w") as f:
                json.dump(data_batch, f)
        print(f"\n\n✅ Total: {sum(counts.values())} items saved!")
        print(f"Files: {len(os.listdir('generated'))}")
        break
