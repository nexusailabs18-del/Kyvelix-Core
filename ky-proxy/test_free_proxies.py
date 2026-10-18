import requests
import random

print("🎯 Testing FREE Real Proxies\n")

# Free proxy sources
free_proxy_sources = [
    "https://raw.githubusercontent.com/TheSpeedX/PROXY-List/master/http.txt",
    "https://raw.githubusercontent.com/clarketm/proxy-list/master/proxy-list-raw.txt",
    "https://raw.githubusercontent.com/monosans/proxy-list/main/proxies/http.txt",
]

real_proxies = []

# Fetch free proxies
for source in free_proxy_sources:
    try:
        response = requests.get(source, timeout=10)
        proxies = response.text.strip().split('\n')
        real_proxies.extend(proxies[:10])
        print(f"✅ Got {len(proxies[:10])} from {source.split('/')[-1]}")
    except:
        print(f"❌ Failed: {source.split('/')[-1]}")

print(f"\nTotal free proxies: {len(real_proxies)}")

# Test them
for proxy in real_proxies[:20]:
    try:
        proxies = {"http": f"http://{proxy}", "https": f"http://{proxy}"}
        response = requests.get("http://httpbin.org/ip", proxies=proxies, timeout=5)
        print(f"✅ {proxy} → {response.json()['origin']}")
    except:
        print(f"❌ {proxy} → Dead")
