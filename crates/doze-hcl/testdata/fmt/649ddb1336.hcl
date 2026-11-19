app "storefront" {
  dir  = "apps/storefront"
  run  = "bun run vite --host $HOST --port $PORT --strictPort"
  port = 5173
}
