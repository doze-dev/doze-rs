
stack = "blocks"

defaults {
  names = false
}

app "api" {
  run = "sleep 600"
}

app "old" {
  run = "sleep 600"
}

app "same" {
  run = "sleep 600"
}
