from llama_cpp import Llama
import time
import json
import random

print("🧠 COMPLETE Background AI Generation\n")

# Load models
print("Loading TinyLlama (TLS + Headers)...")
tiny = Llama("models/text/tinyllama-1.1b-chat-v1.0.Q4_K_M.gguf", n_ctx=64, n_threads=12)

print("Loading Phi-2 (UA + Behavior)...")
phi2 = Llama("models/text/phi-2.Q4_K_M.gguf", n_ctx=64, n_threads=12)

print("Loading Mistral (Cookies + Geo)...")
mistral = Llama("models/text/mistral-7b-v0.1.Q4_K_M.gguf", n_ctx=64, n_threads=12)

print("\n✅ All models loaded!\n")

# Complete generation system
counts = {
    "tls_fingerprints": 0,
    "user_agents": 0,
    "cookies": 0,
    "headers": 0,
    "behavior": 0,
    "geolocation": 0,
}

browsers = ["Chrome 120", "Chrome 119", "Firefox 121", "Safari 17", "Edge 120", "Opera 100"]

while True:
    try:
        # 1. TLS Fingerprint (TinyLlama)
        browser = random.choice(browsers)
        tls = tiny(f"Generate TLS ciphers for {browser}:", max_tokens=10)
        counts["tls_fingerprints"] += 1
        
        # 2. User-Agent (Phi-2)
        ua = phi2(f"Generate {random.choice(browsers)} User-Agent:", max_tokens=15)
        counts["user_agents"] += 1
        
        # 3. Cookies (Mistral)
        cookie = mistral("Generate session cookie value:", max_tokens=10)
        counts["cookies"] += 1
        
        # 4. Headers (TinyLlama)
        header = tiny("Generate HTTP accept header:", max_tokens=10)
        counts["headers"] += 1
        
        # 5. Behavior (Phi-2)
        behavior = phi2(f"Mouse speed {random.randint(50,200)} scroll:", max_tokens=10)
        counts["behavior"] += 1
        
        # 6. Geolocation (Mistral)
        geo = mistral(f"Generate location for {random.choice(['US','UK','IN'])}:", max_tokens=10)
        counts["geolocation"] += 1
        
        # Show progress
        if counts["tls_fingerprints"] % 5 == 0:
            total = sum(counts.values())
            print(f"[{time.strftime('%H:%M:%S')}] Total: {total} | {counts}")
            print()
        
    except KeyboardInterrupt:
        print(f"\n✅ FINAL: Generated {sum(counts.values())} items!")
        print(json.dumps(counts, indent=2))
        break
    except Exception as e:
        print(f"Error: {str(e)[:40]}")
        time.sleep(1)

