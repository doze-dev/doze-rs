app "fulfilment" {
  dir   = "services/fulfilment"
  run   = "uv run --frozen python -m fulfilment"
  port  = 8005
  ready = http("/healthz")
  after = [task.install-fulfilment]
}

app "storefront" {
  dir   = "apps/storefront"
  run   = "bun run dev"
  port  = 5173
  ready = http("/")
  after = [task.install-storefront]
  env   = { GATEWAY_URL = app.gateway.url }
}
