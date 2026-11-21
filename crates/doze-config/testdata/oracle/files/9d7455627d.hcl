# 0.2
app "api" {
  dir   = "../api"
  run   = "go run ./cmd/api"
  port  = 8080
  after = [module.app]

  setup    = "go run ./cmd/migrate up"
  on_ready = "./scripts/notify-ready.sh"
  stop     = "./scripts/drain.sh"

  ready = http("/health/ready")
}
