package main

/*
#cgo LDFLAGS: -L/home/kumarr/kyvelix-venv/lib/python3.14/site-packages/playwright/kywright_rust/target/release -lkywright_filter -ldl -lm
#cgo CFLAGS: -I/home/kumarr/kyvelix-venv/lib/python3.14/site-packages/playwright/kywright_rust
#include <stdlib.h>

extern char* dom_parse(const char* input);
extern char* dom_extract_urls(const char* input);
extern void dom_free_string(char* s);
*/
import "C"

import (
	"encoding/json"
	"unsafe"
)

type ParsedDOM struct {
	Title string   `json:"title"`
	Links []string `json:"links"`
}

func rustDOMParse(html string) ParsedDOM {
	cIn := C.CString(html)
	defer C.free(unsafe.Pointer(cIn))

	cOut := C.dom_parse(cIn)
	if cOut == nil {
		// Fallback to tunnel
		if tunnel, err := getOrCreateTunnel(); err == nil {
			if body, terr := tunnel.Fetch("ky://dom/" + html); terr == nil && body != nil {
				var parsed ParsedDOM
				json.Unmarshal(body, &parsed)
				return parsed
			}
		}
		return ParsedDOM{}
	}
	defer C.dom_free_string(cOut)

	var parsed ParsedDOM
	json.Unmarshal([]byte(C.GoString(cOut)), &parsed)
	return parsed
}

func rustExtractURLs(html string) []string {
	cIn := C.CString(html)
	defer C.free(unsafe.Pointer(cIn))

	cOut := C.dom_extract_urls(cIn)
	if cOut == nil {
		return nil
	}
	defer C.dom_free_string(cOut)

	var urls []string
	json.Unmarshal([]byte(C.GoString(cOut)), &urls)
	return urls
}
