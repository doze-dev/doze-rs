stack = "namestest"

defaults {
  names = true
}

app "api" {
  run  = "python3 -c \"import socketserver as s, http.server as h, os; s.TCPServer((os.environ['HOST'], int(os.environ['PORT'])), h.SimpleHTTPRequestHandler).serve_forever()\""
  port = 18431
}
