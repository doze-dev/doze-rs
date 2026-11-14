stack = "shop"
defaults { names = true }
app "web" {
  run     = "pnpm dev"
  port    = 3000
  domains = ["dev.acme.com", "*.dev.acme.com"]
}
