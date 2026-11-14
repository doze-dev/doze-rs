stack = "shop"

defaults {
  names = true
}

app "api" {
  run  = "./api"
  port = 8080
}

app "legacy" {
  run = "./legacy"
}
