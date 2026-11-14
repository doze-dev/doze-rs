stack = "shop"

# package.json depends on pg
module postgres "db" {
  version = 18
  port    = 5432
}

# docker-compose.yml has a mail service (axllent/mailpit)
service "mail" {
  # a container: a container is just a command, and `run` stays in the foreground
  run  = "docker run --rm -p $HOST:$PORT:1025 axllent/mailpit"
  port = 1025
}

# package.json has a dev script
app "shop" {
  run = "npm run dev"
}
