stack = "larder"

app "catalog" {                        # your code
  run   = "go run ./services/catalog"
  port  = 8002
  after = [task.migrate-catalog]
  env   = { DATABASE_URL = module.postgres.url }
}

module postgres "postgres" {           # from the doze registry
  version = 18
}

service "mailpit" {                    # anything else
  run  = "mailpit --smtp $HOST:$PORT"
  port = 2525
}

task "migrate-catalog" {               # runs once, then gets out of the way
  run    = "go run ./platform/cmd/migrate services/catalog/migrations"
  inputs = ["services/catalog/migrations/*.sql"]
}
