stack = "blocks"

defaults {
  names = false
}

app "api" {
  run = "sleep 601"
}

app "same" {
  run = "sleep 600"
}

service "mail" {
  run = "sleep 600"
}
