package kywrightfilter

/*
#cgo LDFLAGS: -L../../kywright_rust/target/release -lkywright_filter -ldl -lm
#cgo CFLAGS: -I../../kywright_rust
#include <stdlib.h>
#include "kywright_filter.h"
*/
import "C"

import (
	"errors"
	"runtime"
	"sync"
	"unsafe"
)

var ErrFilterFailed = errors.New("kywrightfilter: filter operation failed")
var ErrSessionClosed = errors.New("kywrightfilter: session already closed")

func FilterHTML(html string) (string, error) {
	cInput := C.CString(html)
	defer C.free(unsafe.Pointer(cInput))

	cOut := C.filter_html(cInput)
	if cOut == nil {
		return "", ErrFilterFailed
	}
	defer C.kywright_free_string(cOut)

	return C.GoString(cOut), nil
}

type Session struct {
	mu     sync.Mutex
	handle *C.KywrightSession
	closed bool
}

func NewSession() (*Session, error) {
	h := C.kywright_session_new()
	if h == nil {
		return nil, errors.New("kywrightfilter: failed to allocate session")
	}
	s := &Session{handle: h}
	runtime.SetFinalizer(s, (*Session).Close)
	return s, nil
}

func (s *Session) Feed(chunk string) error {
	s.mu.Lock()
	defer s.mu.Unlock()
	if s.closed {
		return ErrSessionClosed
	}

	cChunk := C.CString(chunk)
	defer C.free(unsafe.Pointer(cChunk))

	if rc := C.kywright_session_feed(s.handle, cChunk); rc != 0 {
		return ErrFilterFailed
	}
	return nil
}

func (s *Session) TakeOutput() (string, error) {
	s.mu.Lock()
	defer s.mu.Unlock()
	if s.closed {
		return "", ErrSessionClosed
	}

	cOut := C.kywright_session_take_output(s.handle)
	if cOut == nil {
		return "", ErrFilterFailed
	}
	defer C.kywright_free_string(cOut)
	return C.GoString(cOut), nil
}

func (s *Session) Finish() (string, error) {
	s.mu.Lock()
	defer s.mu.Unlock()
	if s.closed {
		return "", ErrSessionClosed
	}

	cOut := C.kywright_session_finish(s.handle)
	if cOut == nil {
		return "", ErrFilterFailed
	}
	defer C.kywright_free_string(cOut)
	return C.GoString(cOut), nil
}

func (s *Session) Close() {
	s.mu.Lock()
	defer s.mu.Unlock()
	if s.closed {
		return
	}
	s.closed = true
	C.kywright_session_free(s.handle)
	s.handle = nil
	runtime.SetFinalizer(s, nil)
}
