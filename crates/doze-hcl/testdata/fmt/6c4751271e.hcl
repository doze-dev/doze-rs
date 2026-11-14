module postgres "db" {
  version    = 18
  extensions = ["uuid-ossp", "pg_trgm"]

  extension "vector" { version = "0.7.0" }
  extension "hstore" { schema = "extensions" }
}
