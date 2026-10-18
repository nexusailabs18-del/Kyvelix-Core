from llama_cpp import Llama
import requests
import time

print("🧠 AI + Web Search + Proxy Integration Test\n")

# Load TinyLlama (fastest)
llm = Llama("models/text/tinyllama-1.1b-chat-v1.0.Q4_K_M.gguf", n_ctx=256, n_threads=12)

def web_search(query):
    """DuckDuckGo search"""
    try:
        response = requests.get(
            "https://api.duckduckgo.com/",
            params={"q": query, "format": "json", "no_html": 1},
            timeout=5
        )
        data = response.json()
        results = []
        for topic in data.get("RelatedTopics", [])[:3]:
            if "Text" in topic:
                results.append(topic["Text"])
        return results
    except:
        return []

def generate_with_search(prompt):
    """AI + Web Search combined"""
    search_results = web_search(prompt)
    context = "\n".join(search_results[:3]) if search_results else "No search results"
    
    full_prompt = f"{prompt}\n\nContext:\n{context}\n\nGenerate output:"
    output = llm(full_prompt, max_tokens=50, temperature=0.7)
    return output['choices'][0]['text'].strip()

# Test 1: Generate TLS with search
print("=== 1. TLS Generation (AI + Search) ===")
start = time.time()
tls = generate_with_search("Latest TLS cipher suites Chrome 2026")
print(f"TLS: {tls[:80]}")
print(f"Time: {time.time()-start:.2f}s\n")

# Test 2: Generate UA with search
print("=== 2. User-Agent (AI + Search) ===")
start = time.time()
ua = generate_with_search("Latest Chrome User-Agent string 2026")
print(f"UA: {ua[:80]}")
print(f"Time: {time.time()-start:.2f}s\n")

# Test 3: Use in proxy request
print("=== 3. Using Generated Data in Proxy ===")
proxies = {
    "http": "http://admin:kyprox123@127.0.0.1:8080",
    "https": "http://admin:kyprox123@127.0.0.1:8080",
}

headers = {
    "User-Agent": ua if ua else "Mozilla/5.0 (Windows NT 10.0; Win64; x64) Chrome/120.0.0.0",
    "Accept": "text/html,application/xhtml+xml",
}

try:
    response = requests.get("http://httpbin.org/headers", proxies=proxies, headers=headers, timeout=10)
    print(f"✅ Proxy response: {response.status_code}")
    print(f"Sent UA: {headers['User-Agent'][:60]}")
except Exception as e:
    print(f"❌ Proxy error: {str(e)[:50]}")

print("\n✅ Full AI + Search + Proxy test complete!")
