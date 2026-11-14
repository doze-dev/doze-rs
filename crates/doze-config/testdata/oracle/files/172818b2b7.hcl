task "migrate-catalog" {
  run    = "go run ./platform/cmd/migrate services/catalog/migrations"
  inputs = ["services/catalog/migrations/*.sql"]
  env    = { DATABASE_URL = "postgres://catalog:catalog@${module.postgres.address}/postgres?sslmode=disable" }
}

app "catalog" {
  run   = "go run ./services/catalog"
  port  = 8002
  after = [task.migrate-catalog]
}
