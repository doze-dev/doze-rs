module postgres "postgres" {
  version = 18
  port    = 5432

  role "catalog" { password = "catalog" }
  role "orders" { password = "orders" }

  schema "catalog" { owner = "catalog" }
  schema "orders" { owner = "orders" }
}

app "catalog" {
  run  = "go run ./services/catalog"
  port = 8002
  env = {
    DATABASE_URL = "postgres://catalog:catalog@${module.postgres.address}/postgres?sslmode=disable"
  }
}
