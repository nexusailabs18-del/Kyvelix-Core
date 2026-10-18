import json
import random

print("📊 Generating datasets for ALL components...\n")

# 1. Header Dataset
headers_data = []
browsers = ["Chrome", "Firefox", "Safari", "Edge", "Opera"]
platforms = ["Windows 10", "Windows 11", "macOS", "Ubuntu Linux"]

for i in range(50_000):
    browser = random.choice(browsers)
    platform = random.choice(platforms)
    prompt = f"Generate HTTP headers for {browser} on {platform}"
    completion = f"User-Agent: Mozilla/5.0 ({platform}) {browser}/120.0\nAccept: text/html,application/xhtml+xml\nAccept-Language: en-US,en;q=0.9\nAccept-Encoding: gzip, deflate, br"
    headers_data.append({"prompt": prompt, "completion": completion})

with open("datasets/headers_dataset.jsonl", "w") as f:
    for item in headers_data:
        f.write(json.dumps(item) + "\n")
print(f"✅ Headers: {len(headers_data)} samples")

# 2. Cookie Dataset
cookies_data = []
for i in range(30_000):
    prompt = f"Generate realistic session cookies for {random.choice(['Amazon', 'Google', 'Facebook', 'Twitter'])}"
    completion = f"session_id={random.randint(100000,999999)}; csrf_token={random.randint(100000,999999)}; tracking_id={random.randint(100000,999999)}"
    cookies_data.append({"prompt": prompt, "completion": completion})

with open("datasets/cookies_dataset.jsonl", "w") as f:
    for item in cookies_data:
        f.write(json.dumps(item) + "\n")
print(f"✅ Cookies: {len(cookies_data)} samples")

# 3. Behavior Dataset
behavior_data = []
for i in range(30_000):
    prompt = "Generate human-like browsing behavior pattern"
    completion = f"Mouse speed: {random.randint(50,200)}px/s\nScroll speed: {random.randint(100,500)}px/s\nTyping speed: {random.randint(50,100)}wpm\nDwell time: {random.randint(2,30)}s"
    behavior_data.append({"prompt": prompt, "completion": completion})

with open("datasets/behavior_dataset.jsonl", "w") as f:
    for item in behavior_data:
        f.write(json.dumps(item) + "\n")
print(f"✅ Behavior: {len(behavior_data)} samples")

# 4. Geolocation Dataset
geo_data = []
cities = [("New York", "US", 40.7128, -74.0060), ("London", "UK", 51.5074, -0.1278), ("Mumbai", "IN", 19.0760, 72.8777)]
for i in range(20_000):
    city, country, lat, lon = random.choice(cities)
    prompt = f"Generate geolocation for {city}"
    completion = f"Country: {country}\nCity: {city}\nLatitude: {lat}\nLongitude: {lon}"
    geo_data.append({"prompt": prompt, "completion": completion})

with open("datasets/geolocation_dataset.jsonl", "w") as f:
    for item in geo_data:
        f.write(json.dumps(item) + "\n")
print(f"✅ Geolocation: {len(geo_data)} samples")

# 5. User-Agent Dataset
ua_data = []
for i in range(50_000):
    browser = random.choice(browsers)
    platform = random.choice(platforms)
    prompt = f"Generate User-Agent for {browser} on {platform}"
    completion = f"Mozilla/5.0 ({platform}) AppleWebKit/537.36 (KHTML, like Gecko) {browser}/120.0 Safari/537.36"
    ua_data.append({"prompt": prompt, "completion": completion})

with open("datasets/ua_dataset.jsonl", "w") as f:
    for item in ua_data:
        f.write(json.dumps(item) + "\n")
print(f"✅ User-Agents: {len(ua_data)} samples")

print("\n🎉 All datasets generated!")
print(f"Total samples: {len(headers_data) + len(cookies_data) + len(behavior_data) + len(geo_data) + len(ua_data)}")
