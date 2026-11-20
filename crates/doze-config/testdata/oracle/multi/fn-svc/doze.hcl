service "s" {
  run   = "x"
  port  = 8080
  ready = lgo("up")
}
