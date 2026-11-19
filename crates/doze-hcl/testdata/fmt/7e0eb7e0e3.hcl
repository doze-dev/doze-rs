# shop — the fixture stack.
#
# A small but real project that doze's own tests and measurements run against:
# three Go programs and four modules, wired together by references (see
# docs/design/next/phases.md).
#
#   doze up            from this directory
#   doze up --var names=false    on a machine without `doze dns-setup`,
#                                  or beside other stacks you must not disturb

stack = "shop"

variable "domains" {
  type        = bool
  default     = true
  description = "Publish <service>.shop.doze names. Off binds everything on 127.0.0.1."
}

defaults {
  sleep = "2m"
  names = var.domains
}

# ── modules ───────────────────────────────────────────────────────────────

module postgres "db" {
  version = 16
  port    = 5432
}

module valkey "cache" {
  version = 9
  port    = 6379
}

module kafka "events" {
  version = 4
  port    = 9092

  topic "orders" {
    partitions = 3
  }
}

module aws "cloud" {
  port = 4566

  bucket "uploads" {}
  queue "orders" {}
}

# ── services and tasks ────────────────────────────────────────────────────

# Something that is not in the registry: fetched once, pinned in doze.lock,
# run from the archive. It sleeps when nothing is connected.
service "mailpit" {
  download = {
    mac       = "https://github.com/axllent/mailpit/releases/download/v1.21.8/mailpit-darwin-arm64.tar.gz"
    mac_intel = "https://github.com/axllent/mailpit/releases/download/v1.21.8/mailpit-darwin-amd64.tar.gz"
    linux     = "https://github.com/axllent/mailpit/releases/download/v1.21.8/mailpit-linux-amd64.tar.gz"
    linux_arm = "https://github.com/axllent/mailpit/releases/download/v1.21.8/mailpit-linux-arm64.tar.gz"
  }
  run  = "./mailpit --smtp $HOST:$PORT --listen $HOST:$((PORT + 1)) --database $DATA/mail.db"
  port = 1025
}

# Runs before the app that lists it, when it has never succeeded or its inputs
# changed.
task "migrate" {
  run    = "ls migrations/*.sql | wc -l"
  inputs = ["migrations/*.sql"]
  env    = { DATABASE_URL = module.db.url }
}

# Only when asked: `doze task seed`.
task "seed" {
  run    = "echo seeding the demo data"
  manual = true
  after  = [task.migrate]
}

# ── apps ──────────────────────────────────────────────────────────────────

app "api" {
  run  = "go run ./cmd/api"
  port = 8080

  env = {
    DATABASE_URL  = module.db.url
    CACHE_ADDR    = module.cache.address
    KAFKA_BROKERS = module.events.address
    SMTP_ADDR     = service.mailpit.address
  }

  ready         = http("/healthz")
  ready_timeout = "90s" # the first `go run` compiles the program

  after = [task.migrate]
}

app "worker" {
  run = "go run ./cmd/worker"

  env = {
    KAFKA_BROKERS = module.events.address
    AWS_ENDPOINT  = module.cloud.url
    ORDERS_QUEUE  = "orders"
    UPLOADS       = "uploads"
  }
}

app "web" {
  run  = "go run ./cmd/web"
  port = 8090

  env = {
    API_URL = "http://${app.api.address}"
  }

  ready         = http("/healthz")
  ready_timeout = "90s"
}
