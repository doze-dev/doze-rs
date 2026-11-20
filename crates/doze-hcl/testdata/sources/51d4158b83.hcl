module postgres "db" {
  version    = 17
  extensions = ["vector", "pg_cron", "postgis"]
}

module postgres "metrics" {
  version = 17
  extension "timescaledb" { version = "2.31" }   # a pin, when it matters
}
