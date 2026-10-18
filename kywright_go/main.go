package main

import (
	"fmt"
	"io"
	"log"
	"net/url"
	"net/http"
	"strings"
	"sync"
	"time"
)

var globalTunnel *Tunnel
const proxyURL = "http://admin:kyprox123@127.0.0.1:8081"

var boringClient = &http.Client{
	Timeout: 30 * time.Second,
	Transport: &http.Transport{
		Proxy:               http.ProxyURL(parseProxyURL(proxyURL)),
		MaxIdleConns:        500,
		MaxIdleConnsPerHost: 100,
		IdleConnTimeout:     120 * time.Second,
		DisableCompression:  false,
		ForceAttemptHTTP2:   false,
	},
}

var cookieCache = make(map[string]map[string]string)
var cookieMutex sync.RWMutex

var protectedDomains = map[string]bool{
	"amazon.com":     true,
	"amazon.in":      true,
	"cloudflare.com": true,
	"datadome.com":   true,
	"akamai.com":     true,
	"reddit.com":     true,
	"twitter.com":    true,
	"x.com":          true,
	"github.com":     true,
	"wikipedia.org":  true,
	"wikimedia.org":  true,
}

func isProtectedDomain(rawURL string) bool {
	for domain := range protectedDomains {
		if strings.Contains(rawURL, domain) {
			return true
		}
	}
	return false
}

func getCachedCookies(rawURL string) map[string]string {
	cookieMutex.RLock()
	defer cookieMutex.RUnlock()
	return cookieCache[rawURL]
}

func setCachedCookies(rawURL string, cookies map[string]string) {
	cookieMutex.Lock()
	defer cookieMutex.Unlock()
	cookieCache[rawURL] = cookies
}

func boringFetch(rawURL string, cookies map[string]string) (string, error) {
	// Try KY Tunnel first

	var lastErr error

	// Retry up to 3 times

	// Retry up to 3 times
	for attempt := 0; attempt < 3; attempt++ {
		req, err := http.NewRequest("GET", rawURL, nil)
		if err != nil {
			return "", err
		}

		// Use randomized headers if Redis available, else fixed
		headers := getBoringHeaders()
		for key, value := range headers {
			req.Header.Set(key, value)
		}

		for name, value := range cookies {
			req.AddCookie(&http.Cookie{Name: name, Value: value})
		}

		resp, err := boringClient.Do(req)
		if err != nil {
			lastErr = err
			continue
		}
		defer resp.Body.Close()

		if resp.StatusCode >= 500 {
			lastErr = fmt.Errorf("server error: %d", resp.StatusCode)
			continue
		}

		body, _ := io.ReadAll(resp.Body)
		return string(body), nil
	}

	return "", lastErr
}
func parseProxyURL(proxyURL string) *url.URL {
	u, err := url.Parse(proxyURL)
	if err != nil {
		return nil
	}
	return u
}

func getOrCreateTunnel() (*Tunnel, error) {
	if globalTunnel != nil {
		return globalTunnel, nil
	}
	t, err := NewTunnel("127.0.0.1")
	if err != nil {
		return nil, err
	}
	globalTunnel = t
	return t, nil
}

func getBoringHeaders() map[string]string {
	return map[string]string{
		"User-Agent":      "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/120.0.0.0 Safari/537.36",
		"Accept":          "text/html,application/xhtml+xml,application/xml;q=0.9,*/*;q=0.8",
		"Accept-Language": "en-US,en;q=0.9",
	}
}

func applyFilter(html, filterType string) string {
	switch filterType {
	case "basic":
		return rustFilter(html)
	case "adv":
		return rustFilter(html)
	default:
		return html
	}
}

func fetchURLWithFilterAndFetcher(rawURL, filterType, fetcher string) string {
	// Manual fetcher override
	if fetcher == "tls" {
		html, freshCookies, err := tlsFetch(rawURL)
		if err == nil {
			if len(freshCookies) > 0 {
				setCachedCookies(rawURL, freshCookies)
			}
			return applyFilter(html, filterType)
		}
	}

	if fetcher == "boringssl" || fetcher == "" {
		if isProtectedDomain(rawURL) {
			html, freshCookies, err := tlsFetch(rawURL)
			if err == nil {
				if len(freshCookies) > 0 {
					setCachedCookies(rawURL, freshCookies)
				}
				return applyFilter(html, filterType)
			}

			// Fallback to boring if TLS fails
			if cookies := getCachedCookies(rawURL); len(cookies) > 0 {
				html, err := boringFetch(rawURL, cookies)
				if err == nil && !isBlocked(html) {
					return applyFilter(html, filterType)
				}
			}
		}

		html, err := boringFetch(rawURL, nil)
		if err == nil && !isBlocked(html) {
			return applyFilter(html, filterType)
		}

		html2, _, _ := tlsFetch(rawURL)
		if html2 != "" {
			return applyFilter(html2, filterType)
		}

		return html
	}

	// Default auto
	return fetchURLWithFilter(rawURL, filterType)
}

func fetchURLWithFilter(rawURL, filterType string) string {
	if isProtectedDomain(rawURL) {
		// Try TLS client first for protected domains
		html, freshCookies, err := tlsFetch(rawURL)
		if err == nil {
			if len(freshCookies) > 0 {
				setCachedCookies(rawURL, freshCookies)
			}
			return applyFilter(html, filterType)
		}

		// Fallback to boring with cookies
		if cookies := getCachedCookies(rawURL); len(cookies) > 0 {
			html, err := boringFetch(rawURL, cookies)
			if err == nil && !isBlocked(html) {
				return applyFilter(html, filterType)
			}
		}
	}

	html, err := boringFetch(rawURL, nil)
	if err == nil && !isBlocked(html) {
		return applyFilter(html, filterType)
	}

	html2, _, _ := tlsFetch(rawURL)
	if html2 != "" {
		return applyFilter(html2, filterType)
	}

	return html
}

func fetchURL(rawURL string) string {
	return fetchURLWithFilter(rawURL, "basic")
}

func main() {
	// Initialize TLS client once at startup
	if err := initTLSClient(); err != nil {
		fmt.Println("TLS client init error:", err)
	}

	// Initialize Redis once at startup
	if err := initRedis(); err != nil {
		fmt.Println("Redis init error:", err)
	} else {
		// Load header pool into Redis once
		if err := loadLargeHeaderPoolToRedis(); err != nil {
			fmt.Println("Header pool load error:", err)
		}
	}

	http.HandleFunc("/fetch", func(w http.ResponseWriter, r *http.Request) {
		rawURL := r.URL.Query().Get("url")
		if rawURL == "" {
			http.Error(w, "Missing url", http.StatusBadRequest)
			return
		}

		filterType := r.URL.Query().Get("filter")
		if filterType == "" {
			filterType = "basic"
		}

		fetcher := r.URL.Query().Get("fetcher")
		body := fetchURLWithFilterAndFetcher(rawURL, filterType, fetcher)
		w.Header().Set("Content-Type", "text/html")
		w.Write([]byte(body))
	})

	http.HandleFunc("/fetch/batch", handleBatch)
	http.HandleFunc("/crawl", handleCrawl)

	http.HandleFunc("/load_headers", func(w http.ResponseWriter, r *http.Request) {
		if err := loadLargeHeaderPoolToRedis(); err != nil {
			http.Error(w, err.Error(), http.StatusInternalServerError)
			return
		}

		chromeCount := redisClient.SCard(ctx, "ua:chrome").Val()
		firefoxCount := redisClient.SCard(ctx, "ua:firefox").Val()
		langCount := redisClient.SCard(ctx, "accept_languages").Val()
		encCount := redisClient.SCard(ctx, "accept_encodings").Val()

		fmt.Fprintf(w, "Chrome UAs: %d\nFirefox UAs: %d\nLanguages: %d\nEncodings: %d\n",
			chromeCount, firefoxCount, langCount, encCount)
	})

	fmt.Println("KYWRIGHT Hybrid Fetcher + Batch + Crawler on :8080")

	http.HandleFunc("/tunnel", func(w http.ResponseWriter, r *http.Request) {
		url := r.URL.Query().Get("url")
		if url == "" {
			http.Error(w, "missing url", 400)
			return
		}

		t, err := NewTunnel("127.0.0.1")
		if err != nil {
			http.Error(w, err.Error(), 500)
			return
		}
		defer t.Close()

		start := time.Now()
		body, err := t.Fetch(url)
		elapsed := time.Since(start)

		if err != nil {
			http.Error(w, err.Error(), 502)
			return
		}

		w.Header().Set("Content-Type", "text/html")
		w.Header().Set("X-Tunnel-Time", elapsed.String())
		w.Write(body)
	})
	log.Fatal(http.ListenAndServe(":8080", nil))
}
