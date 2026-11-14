module postgres "db" { version = 16 }
app "api" {
  run = "x"
  env = { DATABASE_URL = postgres.db.url }
}