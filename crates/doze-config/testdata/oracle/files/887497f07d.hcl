
module fake "a" { version = 1 }
service "s" {
  run   = "true"
  after = [started(module.a)]
}
