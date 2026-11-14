
stack = "shop"
defaults { names = true }
module netmod "events" {
  version = 1
  port    = 9092
}
module fake "sync-events" {
  version = 1
  port    = 7000
}
