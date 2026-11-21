module postgres "db" {
  version = 16
}
task "seed" {
  run    = "y"
  manual = true
}
