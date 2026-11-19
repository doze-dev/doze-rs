# local.override.doze.hcl
module postgres "db" {
  port = 5499
}

app "admin" {
  enabled = false
}

module kafka "events" {
  sleep = "never"
}
