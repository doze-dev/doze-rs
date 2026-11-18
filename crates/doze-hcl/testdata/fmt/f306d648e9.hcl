# my stack — hand written, and it should stay that way
stack = "shop"

# the database
module postgres "db" {
  version = 16 # pinned
  port    = 5432
}

app "api" {
  run = "go run ./cmd/api"
}
