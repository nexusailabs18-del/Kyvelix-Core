from playwright.sync_api import sync_playwright

print("🎯 AKAMAI BYPASS - Fixed Proxy Auth\n")

with sync_playwright() as p:
    # Launch with proxy (no auth in URL for Playwright)
    browser = p.chromium.launch(
        proxy={"server": "http://127.0.0.1:8081"},
        headless=True
    )
    
    # Create context with HTTP auth credentials
    context = browser.new_context(
        http_credentials={"username": "admin", "password": "kyprox123"}
    )
    
    page = context.new_page()
    
    print("Testing Akamai...")
    try:
        page.goto("https://www.akamai.com", timeout=30000)
        print(f"✅ AKAMAI BYPASSED!")
        print(f"Title: {page.title()[:60]}")
    except Exception as e:
        print(f"❌ Failed: {str(e)[:50]}")
    
    print("\nTesting Cloudflare...")
    try:
        page.goto("https://www.cloudflare.com", timeout=30000)
        print(f"✅ Cloudflare: {page.title()[:40]}")
    except:
        print("❌ Cloudflare failed")
    
    browser.close()
