stack = "svc"

defaults {
  names = false
}

service "mail" {
  run   = "%s"
  port  = %d
  sleep = "2s"
}
