module postgres "db" {
  version    = 18
  extensions = ["uuid-ossp", "pg_stat_statements", "pg_trgm"]
}
