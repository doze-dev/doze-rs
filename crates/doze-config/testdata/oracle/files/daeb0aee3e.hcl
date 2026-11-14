task "migrate-catalog" {
  run    = "go run ./platform/cmd/migrate services/catalog/migrations"
  inputs = ["services/catalog/migrations/*.sql"]
  env    = { DATABASE_URL = "postgres://catalog:catalog@${module.postgres.address}/postgres?sslmode=disable" }
}

task "install-fulfilment" {
  dir    = "services/fulfilment"
  run    = "uv sync --frozen"
  inputs = ["pyproject.toml", "uv.lock"]
}

# Only when asked.
task "seed-catalog" {
  run    = "go run ./platform/cmd/migrate services/catalog/seed"
  manual = true
  after  = [task.migrate-catalog]
  env    = { DATABASE_URL = "postgres://catalog:catalog@${module.postgres.address}/postgres?sslmode=disable" }
}
