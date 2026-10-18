from playwright.sync_api import sync_playwright

print("🎯 AKAMAI BYPASS TEST (Real Chrome)\n")

with sync_playwright() as p:
    browser = p.chromium.launch(
        proxy={"server": "http://127.0.0.1:8081", "username": "admin", "password": "kyprox123"},
        headless=True
    )
    page = browser.new_page()
    
    print("Testing Akamai with REAL Chrome TLS...")
    try:
        page.goto("https://www.akamai.com", timeout=30000)
        print(f"✅ AKAMAI BYPASSED! Title: {page.title()[:50]}")
    except Exception as e:
        print(f"❌ Failed: {str(e)[:50]}")
    
    browser.close()
