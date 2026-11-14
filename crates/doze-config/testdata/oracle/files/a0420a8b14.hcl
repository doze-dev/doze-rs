
module valkey "events" {
  version = 9
}
module valkey "cache" {
  version  = 9
  wakes_on = [module.events]
}
