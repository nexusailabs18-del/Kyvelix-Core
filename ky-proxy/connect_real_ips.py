import requests
import random

print("🎯 Using Generated IPs as REAL Proxies\n")

# Our generated IPs (from test_real_ips)
generated_ips = [
    "81.0.164.23",    # BT, London
    "24.187.205.144", # Comcast, NY
    "174.230.111.113",# Verizon
    "49.42.151.41",   # Jio, Mumbai
    "3.215.64.105",   # AWS
]

# Try each IP as a direct proxy
for ip in generated_ips:
    print(f"Trying: {ip}")
    proxies = {
        "http": f"http://{ip}:8080",
        "https": f"http://{ip}:8080",
    }
    
    try:
        response = requests.get("http://httpbin.org/ip", proxies=proxies, timeout=5)
        print(f"  ✅ Connected! Target sees: {response.json()['origin']}")
    except Exception as e:
        print(f"  ❌ Failed: {str(e)[:50]}")
    print()

print("=== REALITY CHECK ===")
print("These IPs are NOT real servers.")
print("They don't have port 8080 open.")
print("They're not connected to the internet as proxy servers.")
print()
print("To use them as REAL proxies, you need:")
print("1. Buy proxy service with these IP ranges")
print("2. Or build SIM farm with these carrier IPs")
print("3. Or use VPN with these locations")
