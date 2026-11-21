# 0.1
process "api" {
  cwd     = "../api"
  command = "go run ./cmd/api"
  port    = 8080

  depends_on = { "postgres.app" = "healthy" }

  hooks {
    pre_start  = ["go run ./cmd/migrate up"]
    post_start = ["./scripts/notify-ready.sh"]
    pre_stop   = ["./scripts/drain.sh"]
  }

  health {
    http = "http://localhost:8080/health/ready"
  }
}
