module postgres "app" {
  version          = 18
  port             = 5432
  owner            = "app"
  encoding         = "UTF8"
  locale           = "en_US.UTF-8"
  connection_limit = 50
  comment          = "primary app database"
  shared_buffers   = "256MB"
  max_connections  = 100

  settings = {
    log_min_duration_statement = "200ms"
  }

  role "app" {
    password         = "app"
    login            = true
    createdb         = true
    connection_limit = 20
    member_of        = ["readers"]
  }

  role "readers" {
    login = false
  }

  schema "analytics" {
    owner = "app"
  }

  extension "pg_trgm" {}

  grant {
    role       = "app"
    privileges = ["ALL"]
    schema     = "analytics"
    objects    = "tables"
  }
}
