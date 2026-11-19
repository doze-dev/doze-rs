module fake "db" { port = 5499 }
app "api" {
  enabled = false
  sleep   = "never"
}
