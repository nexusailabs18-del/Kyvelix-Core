package main

import (
	"encoding/json"
	"fmt"
	"net/http"
	"strings"
	"sync"
)

type BatchResult struct {
	URL  string `json:"url"`
	HTML string `json:"html"`
	Err  string `json:"error,omitempty"`
}

func handleBatch(w http.ResponseWriter, r *http.Request) {
	urlList := r.URL.Query().Get("urls")
	if urlList == "" {
		http.Error(w, "no urls provided", http.StatusBadRequest)
		return
	}

	urls := strings.Split(urlList, ",")

	filterType := r.URL.Query().Get("filter")
	if filterType == "" {
		filterType = "none"
	}

	workers := 1
	if wParam := r.URL.Query().Get("workers"); wParam != "" {
		fmt.Sscanf(wParam, "%d", &workers)
	}

	jobs := make(chan string, len(urls))
	results := make(chan BatchResult, len(urls))

	var wg sync.WaitGroup
	for i := 0; i < workers; i++ {
		wg.Add(1)
		go func() {
			defer wg.Done()
			for url := range jobs {
				html := ""
				// Try tunnel fast path first
				if tunnel, err := getOrCreateTunnel(); err == nil {
					if body, terr := tunnel.Fetch(url); terr == nil && body != nil {
						html = string(body)
					}
				}
				if html == "" {
					html = fetchURLWithFilter(url, filterType)
				}
				results <- BatchResult{URL: url, HTML: html}
			}
		}()
	}

	for _, u := range urls {
		jobs <- u
	}
	close(jobs)

	go func() {
		wg.Wait()
		close(results)
	}()

	var out []BatchResult
	for res := range results {
		out = append(out, res)
	}

	w.Header().Set("Content-Type", "application/json")
	json.NewEncoder(w).Encode(out)
}
