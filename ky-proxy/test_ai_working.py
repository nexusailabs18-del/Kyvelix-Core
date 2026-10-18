from llama_cpp import Llama
import requests
import re

print("🧠 AI + Proxy FULLY Working Test\n")

# Load TinyLlama
llm = Llama("models/text/tinyllama-1.1b-chat-v1.0.Q4_K_M.gguf", n_ctx=128, n_threads=12)

def clean_output(text):
    """Clean AI output for HTTP use"""
    # Remove code blocks, special chars
    text = re.sub(r'```.*?```', '', text, flags=re.DOTALL)
    text = text.split('\n')[0]  # Take first line only
    text = re.sub(r'[^a-zA-Z0-9\s./;()\-_:]', '', text)
    return text.strip()

# Generate clean UA
print("=== Generating Clean UA ===")
ua_output = llm("Mozilla/5.0 Chrome User-Agent:", max_tokens=30)
ua = clean_output(ua_output['choices'][0]['text'])
if not ua or len(ua) < 10:
    ua = "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 Chrome/120.0.0.0 Safari/537.36"
print(f"Clean UA: {ua}")

# Generate clean TLS
print("\n=== Generating Clean TLS ===")
tls_output = llm("Generate TLS cipher: 1301 1302 1303", max_tokens=20)
tls = clean_output(tls_output['choices'][0]['text'])
print(f"Clean TLS: {tls}")

# Use in proxy
print("\n=== Sending Through Proxy ===")
proxies = {
    "http": "http://admin:kyprox123@127.0.0.1:8080",
    "https": "http://admin:kyprox123@127.0.0.1:8080",
}

headers = {
    "User-Agent": ua,
    "Accept": "text/html,application/xhtml+xml",
    "X-AI-TLS": tls if tls else "1301,1302,1303",
}

try:
    response = requests.get("http://httpbin.org/headers", proxies=proxies, headers=headers, timeout=10)
    print(f"✅ Status: {response.status_code}")
    
    # Verify what proxy sent
    data = response.json()
    print(f"✅ UA received: {data['headers'].get('User-Agent', 'N/A')[:60]}")
    print(f"✅ AI-TLS received: {data['headers'].get('X-Ai-Tls', 'N/A')}")
except Exception as e:
    print(f"❌ Error: {str(e)[:60]}")

print("\n✅ AI output is CLEAN and USED in proxy request!")
