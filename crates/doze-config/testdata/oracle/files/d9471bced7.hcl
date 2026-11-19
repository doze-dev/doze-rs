
stack = "svc"

defaults {
  names = false
}

service "mail" {
  run      = "%s"
  port     = %d
  on_ready = ["test -f $DATA/ok"]
}
