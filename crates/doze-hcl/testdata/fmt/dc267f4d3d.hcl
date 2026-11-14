service "mailpit" {
  # fetched once, pinned in doze.lock by its checksum, and run from the archive
  download = "https://example.com/mailpit-darwin-arm64.tar.gz" # fill this in
  run      = "./mailpit --port $PORT"
  port     = 1025
}
