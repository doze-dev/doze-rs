app "web" {
  run     = "pnpm dev"
  port    = 3000
  domains = ["dev.acme.com"]
  cert    = "certs/dev.acme.com.crt"
  key     = "certs/dev.acme.com.key"
}
