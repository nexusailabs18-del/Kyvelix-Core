package main

import (
    "encoding/binary"
    "fmt"
    "io"
    "net"
    "net/http"
    "time"
)

const (
    magicLen       = 4
    reqHeaderSize  = 13
    respHeaderSize = 10
)

func main() {
    ln, err := net.Listen("tcp", "127.0.0.1:9090")
    if err != nil {
        panic(err)
    }
    fmt.Println("KY Tunnel Server on 127.0.0.1:9090")

    for {
        conn, err := ln.Accept()
        if err != nil {
            continue
        }
        go handleConn(conn)
    }
}

func handleConn(conn net.Conn) {
    defer conn.Close()
    conn.SetDeadline(time.Now().Add(15 * time.Second))

    for {
        req, err := readRequest(conn)
        if err != nil {
            fmt.Println("read error:", err)
            return
        }

        if string(req[:magicLen]) != "KY01" {
            fmt.Println("bad magic")
            return
        }

        cmd := req[5]
        if cmd == 4 {
            fmt.Println("PING received")
            writeResponse(conn, 0, []byte("PONG"))
            continue
        }

        urlLen := binary.BigEndian.Uint16(req[11:13])
        if len(req) < reqHeaderSize+int(urlLen) {
            fmt.Println("truncated request")
            return
        }
        url := string(req[reqHeaderSize : reqHeaderSize+int(urlLen)])
        fmt.Println("FETCH:", url)

        body, err := fetchURL(url)
        if err != nil {
            fmt.Println("fetch error:", err)
            writeResponse(conn, 1, []byte(err.Error()))
            continue
        }

        fmt.Println("RESPONSE body len:", len(body))
        writeResponse(conn, 0, body)
    }
}

func readRequest(conn net.Conn) ([]byte, error) {
    header := make([]byte, reqHeaderSize)
    if _, err := io.ReadFull(conn, header); err != nil {
        return nil, err
    }
    urlLen := binary.BigEndian.Uint16(header[11:13])
    urlBytes := make([]byte, urlLen)
    if _, err := io.ReadFull(conn, urlBytes); err != nil {
        return nil, err
    }
    return append(header, urlBytes...), nil
}

func writeResponse(conn net.Conn, status byte, body []byte) {
    buf := make([]byte, respHeaderSize+len(body))
    copy(buf[0:4], "KY01")
    buf[4] = status
    buf[5] = 0
    binary.BigEndian.PutUint32(buf[6:10], uint32(len(body)))
    copy(buf[10:], body)
    n, err := conn.Write(buf)
    fmt.Println("WROTE", n, "bytes, err:", err)
}

func fetchURL(url string) ([]byte, error) {
    client := &http.Client{Timeout: 8 * time.Second}
    resp, err := client.Get(url)
    if err != nil {
        return nil, err
    }
    defer resp.Body.Close()
    return io.ReadAll(resp.Body)
}
