task "install-storefront" {
  dir    = "apps/storefront"
  run    = "bun install --frozen-lockfile"
  inputs = ["package.json", "bun.lock"]
}

app "storefront" {
  dir   = "apps/storefront"
  run   = "bun run dev"
  port  = 5173
  after = [task.install-storefront]
}
