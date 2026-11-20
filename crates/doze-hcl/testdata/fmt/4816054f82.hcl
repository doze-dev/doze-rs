service "mailpit" {
  download = {
    mac       = "https://github.com/axllent/mailpit/releases/download/v1.21.8/mailpit-darwin-arm64.tar.gz"
    mac_intel = "https://github.com/axllent/mailpit/releases/download/v1.21.8/mailpit-darwin-amd64.tar.gz"
    linux     = "https://github.com/axllent/mailpit/releases/download/v1.21.8/mailpit-linux-amd64.tar.gz"
    linux_arm = "https://github.com/axllent/mailpit/releases/download/v1.21.8/mailpit-linux-arm64.tar.gz"
  }
  run     = "./mailpit --smtp $HOST:$PORT --listen 127.0.0.1:8025 --database $DATA/mail.db"
  port    = 2525
  console = "http://127.0.0.1:8025"
}

app "notifications" {
  dir = "services/notifications"
  run = "bun src/index.ts"
  env = { SMTP_ADDR = service.mailpit.address }
}
