app "notifications" {
  dir   = "services/notifications"
  run   = "bun src/index.ts"
  ready = log("consuming")
}
