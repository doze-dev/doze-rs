module postgres "db" {
  version = 18
  extension "vector" {
    version = "0.8.0"
    source  = "https://example.com/pgvector/pg18-aarch64-apple-darwin.tar.gz"
  }
}
