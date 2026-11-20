task "seed-catalog" {
  run    = "go run ./platform/cmd/migrate services/catalog/seed"
  manual = true
  after  = [task.migrate-catalog]
}
