app "a" {
  run   = "x"
  port  = 8080
  ready = lgo("up")
}
