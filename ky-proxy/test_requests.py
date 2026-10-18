import requests

print("🧪 Testing with Python Requests (NOT curl)\n")

# Proxy with auth
proxies = {
    "http": "http://admin:kyprox123@127.0.0.1:8080",
    "https": "http://admin:kyprox123@127.0.0.1:8080",
}

# Real Chrome headers
headers = {
    "User-Agent": "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/120.0.0.0 Safari/537.36",
    "Accept": "text/html,application/xhtml+xml,application/xml;q=0.9,image/avif,image/webp,*/*;q=0.8",
    "Accept-Language": "en-US,en;q=0.9",
}

sites = [
    "https://www.amazon.com",
    "https://www.cloudflare.com",
    "https://www.akamai.com",
]

for site in sites:
    print(f"Testing: {site}")
    try:
        response = requests.get(site, proxies=proxies, headers=headers, timeout=15)
        print(f"  ✅ Status: {response.status_code}")
    except Exception as e:
        print(f"  ❌ Error: {str(e)[:50]}")
    print()

print("✅ Done!")
