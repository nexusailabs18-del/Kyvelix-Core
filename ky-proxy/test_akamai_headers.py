import requests

# Exact Chrome headers in correct order
headers = {
    "User-Agent": "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/120.0.0.0 Safari/537.36",
    "Accept": "text/html,application/xhtml+xml,application/xml;q=0.9,image/avif,image/webp,image/apng,*/*;q=0.8,application/signed-exchange;v=b3;q=0.7",
    "Accept-Language": "en-US,en;q=0.9",
    "Accept-Encoding": "gzip, deflate, br",
    "Cache-Control": "max-age=0",
    "Sec-CH-UA": '"Not_A Brand";v="8", "Chromium";v="120", "Google Chrome";v="120"',
    "Sec-CH-UA-Mobile": "?0",
    "Sec-CH-UA-Platform": '"Windows"',
    "Sec-Fetch-Dest": "document",
    "Sec-Fetch-Mode": "navigate",
    "Sec-Fetch-Site": "none",
    "Sec-Fetch-User": "?1",
    "Upgrade-Insecure-Requests": "1",
}

proxies = {"http": "http://admin:kyprox123@127.0.0.1:8081", "https": "http://admin:kyprox123@127.0.0.1:8081"}

print("Testing Akamai with EXACT Chrome headers...")
try:
    r = requests.get("https://www.akamai.com", headers=headers, proxies=proxies, timeout=15)
    print(f"✅ Status: {r.status_code}")
except Exception as e:
    print(f"❌ Failed: {str(e)[:50]}")
