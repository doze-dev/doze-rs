
service "minio" {
  run      = "./minio server $DATA --address :$PORT"
  port     = 9000
  download = {
    mac   = "https://dl.min.io/server/minio/release/darwin-arm64/minio"
    linux = "https://dl.min.io/server/minio/release/linux-amd64/minio"
  }
  setup         = "chmod +x minio"
  on_ready      = ["./mc alias set local", "./mc mb local/uploads"]
  stop          = "pkill minio"
  ready         = http("/minio/health/live")
  ready_timeout = "90s"
  sleep         = "10m"
  console       = "http://localhost:9001"
  restart {
    policy = "always"
  }
}
