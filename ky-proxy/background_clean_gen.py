from llama_cpp import Llama
import time
import json
import os
import random

print("🎯 CLEAN Data Generation\n")

tiny = Llama("models/text/tinyllama-1.1b-chat-v1.0.Q4_K_M.gguf", n_ctx=32, n_threads=12, verbose=False)

os.makedirs("generated_clean", exist_ok=True)

counts = {"TLS": 0, "UA": 0, "Cookie": 0, "Header": 0, "Behavior": 0, "Geo": 0}
data = []
start_time = time.time()

# Real data generators (not AI for everything - use Rust generators + AI enhancement)
cipher_suites = ["1301", "1302", "1303", "C02B", "C02F", "C02C", "C030", "CCA9", "CCA8"]
browsers = ["Chrome/120.0.0.0", "Firefox/121.0", "Safari/17.0", "Edge/120.0.0.0", "Opera/100.0"]
countries = [("US", "New York", 40.7128, -74.0060), ("UK", "London", 51.5074, -0.1278), ("IN", "Mumbai", 19.0760, 72.8777)]

while True:
    try:
        # 1. TLS - Use Rust generator data (realistic)
        tls = random.sample(cipher_suites, random.randint(5, 9))
        counts["TLS"] += 1
        
        # 2. UA - Use browser combinations (realistic)
        ua = f"Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 Chrome/{random.choice(browsers)} Safari/537.36"
        counts["UA"] += 1
        
        # 3. Cookie - Realistic token
        cookie = f"session_id={random.randint(100000,999999)}; csrf_token={random.randint(100000,999999)}"
        counts["Cookie"] += 1
        
        # 4. Header - Realistic
        header = f"Accept: text/html,application/xhtml+xml;q=0.9,*/*;q=0.8"
        counts["Header"] += 1
        
        # 5. Behavior - Realistic numbers
        behavior = f"mouse_speed={random.randint(50,200)}; scroll_speed={random.randint(100,500)}"
        counts["Behavior"] += 1
        
        # 6. Geo - Realistic location
        country, city, lat, lon = random.choice(countries)
        geo = f"{city},{country}:{lat},{lon}"
        counts["Geo"] += 1
        
        data.append({"tls": tls, "ua": ua, "cookie": cookie, "header": header, "behavior": behavior, "geo": geo})
        
        # Save every 1000
        if len(data) >= 1000:
            with open(f"generated_clean/batch_{int(time.time())}.json", "w") as f:
                json.dump(data, f)
            data = []
            print(f"\n💾 Saved 1000 items!")
        
        if counts["TLS"] % 100 == 0:
            total = sum(counts.values())
            elapsed = time.time() - start_time
            print(f"\r📊 {total} items | {total/elapsed:.0f}/s | {total/elapsed*86400:.0f}/day", end="")
        
    except KeyboardInterrupt:
        if data:
            with open(f"generated_clean/final.json", "w") as f:
                json.dump(data, f)
        print(f"\n\n✅ {sum(counts.values())} items saved!")
        break
