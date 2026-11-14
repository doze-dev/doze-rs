defaults {
  ready_timeout = "3m"
}

app "catalog" {
  run           = "go run ./services/catalog"
  port          = 8002
  ready_timeout = "90s"
}
