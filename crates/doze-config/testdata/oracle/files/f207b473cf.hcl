service "s" {
  run = "x"
  env = { P = service.s.port }
}
