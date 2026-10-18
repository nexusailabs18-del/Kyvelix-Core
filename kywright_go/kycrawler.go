package main

import (
	"encoding/json"
	"fmt"
	"net/http"
	"net/url"
	"strings"
	"sync"
	"time"
)

// CrawlResult is what the API returns for one crawled page.
type CrawlResult struct {
	URL   string   `json:"url"`
	HTML  string   `json:"html"`
	Links []string `json:"links"`
	Err   string   `json:"error,omitempty"`
}

// KyCrawler stores crawl state and settings.
type KyCrawler struct {
	workers  int
	maxPages int
	baseHost string
	visited  map[string]bool
	mu       sync.Mutex
}

// NewKyCrawler creates a Kyvelix crawler.
func NewKyCrawler(startURL string, workers, maxPages int) (*KyCrawler, error) {
	parsedURL, err := url.Parse(startURL)
	if err != nil {
		return nil, err
	}

	return &KyCrawler{
		workers:  workers,
		maxPages: maxPages,
		baseHost: parsedURL.Host,
		visited:  make(map[string]bool),
	}, nil
}

// normalizeURL cleans a URL.
func (c *KyCrawler) normalizeURL(rawURL string) string {
	parsed, err := url.Parse(rawURL)
	if err != nil {
		return ""
	}
	parsed.Fragment = ""
	parsed.Scheme = strings.ToLower(parsed.Scheme)
	parsed.Host = strings.ToLower(parsed.Host)
	return parsed.String()
}

// isAllowed checks whether we can crawl this URL.
func (c *KyCrawler) isAllowed(rawURL string) bool {
	parsed, err := url.Parse(rawURL)
	if err != nil {
		return false
	}
	if parsed.Scheme != "http" && parsed.Scheme != "https" {
		return false
	}
	return parsed.Host == c.baseHost
}

// markVisited records a URL and returns true if it is new.
func (c *KyCrawler) markVisited(pageURL string) bool {
	c.mu.Lock()
	defer c.mu.Unlock()

	if c.maxPages > 0 && len(c.visited) >= c.maxPages {
		return false
	}
	if c.visited[pageURL] {
		return false
	}
	c.visited[pageURL] = true
	return true
}

// extractLinks converts raw links into valid absolute URLs.
func (c *KyCrawler) extractLinks(pageURL string, rawLinks []string) []string {
	base, err := url.Parse(pageURL)
	if err != nil {
		return nil
	}

	var links []string
	seen := make(map[string]bool)

	for _, rawLink := range rawLinks {
		rawLink = strings.TrimSpace(rawLink)
		if rawLink == "" {
			continue
		}

		parsedLink, err := url.Parse(rawLink)
		if err != nil {
			continue
		}

		fullURL := base.ResolveReference(parsedLink)
		normalized := c.normalizeURL(fullURL.String())

		if normalized == "" || !c.isAllowed(normalized) {
			continue
		}

		if !seen[normalized] {
			seen[normalized] = true
			links = append(links, normalized)
		}
	}

	return links
}

// crawlWorker processes URLs from the jobs channel.
func (c *KyCrawler) crawlWorker(id int, jobs <-chan string, results chan<- CrawlResult, wg *sync.WaitGroup) {
	defer wg.Done()

	for pageURL := range jobs {
		fmt.Printf("🕷️  [Worker %3d] Crawling: %s\n", id, pageURL)

		html := fetchURL(pageURL)

		rawLinks := rustExtractURLs(html)
		links := c.extractLinks(pageURL, rawLinks)

		fmt.Printf("   ✅ [Worker %3d] Found %d unique links\n", id, len(links))

		results <- CrawlResult{
			URL:   pageURL,
			HTML:  html,
			Links: links,
		}
	}
}

// Crawl starts the crawler and returns all results.
func (c *KyCrawler) Crawl(startURL string) []CrawlResult {
	startURL = c.normalizeURL(startURL)
	if startURL == "" {
		return nil
	}

	if !c.markVisited(startURL) {
		return nil
	}

	fmt.Println("🚀 Starting Kyvelix crawl...")
	fmt.Printf("📋 Config: %d workers | %d max pages\n", c.workers, c.maxPages)
	fmt.Println(strings.Repeat("─", 60))

	jobs := make(chan string, c.workers)
	results := make(chan CrawlResult, c.workers)

	var workerWG sync.WaitGroup
	for i := 1; i <= c.workers; i++ {
		workerWG.Add(1)
		go c.crawlWorker(i, jobs, results, &workerWG)
	}

	startTime := time.Now()
	queue := []string{startURL}
	active := 0

	ticker := time.NewTicker(1 * time.Second)
	defer ticker.Stop()

	var out []CrawlResult

	for {
		if active == 0 && len(queue) == 0 {
			break
		}

		var sendCh chan string
		var sendURL string
		if len(queue) > 0 && active < c.workers {
			sendCh = jobs
			sendURL = queue[0]
		}

		select {
		case sendCh <- sendURL:
			queue = queue[1:]
			active++

		case res := <-results:
			active--
			out = append(out, res)

			for _, link := range res.Links {
				if c.markVisited(link) {
					queue = append(queue, link)
				}
			}

		case <-ticker.C:
			elapsed := time.Since(startTime)
			c.mu.Lock()
			visited := len(c.visited)
			c.mu.Unlock()

			fmt.Printf("📊 Progress: %d/%d pages | %d active | %d queued | %v elapsed\n",
				visited, c.maxPages, active, len(queue), elapsed.Round(time.Millisecond))
		}
	}

	elapsed := time.Since(startTime)
	fmt.Println(strings.Repeat("─", 60))
	fmt.Printf("✅ Crawl complete in %v\n", elapsed.Round(time.Millisecond))

	close(jobs)
	workerWG.Wait()

	return out
}

// handleCrawl is the HTTP endpoint for crawling.
func handleCrawl(w http.ResponseWriter, r *http.Request) {
	startURL := r.URL.Query().Get("url")
	if startURL == "" {
		http.Error(w, "url is required", http.StatusBadRequest)
		return
	}

	workers := 10
	maxPages := 100

	if wParam := r.URL.Query().Get("workers"); wParam != "" {
		fmt.Sscanf(wParam, "%d", &workers)
	}
	if mParam := r.URL.Query().Get("max"); mParam != "" {
		fmt.Sscanf(mParam, "%d", &maxPages)
	}

	crawler, err := NewKyCrawler(startURL, workers, maxPages)
	if err != nil {
		http.Error(w, err.Error(), http.StatusBadRequest)
		return
	}

	results := crawler.Crawl(startURL)

	w.Header().Set("Content-Type", "application/json")
	json.NewEncoder(w).Encode(results)
}
