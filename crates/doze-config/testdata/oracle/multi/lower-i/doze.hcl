stack = "İstanbul"
defaults {
  names = true
}
service "s" {
  run  = "x"
  port = 9000
}
output "o" {
  value = service.s.host
}
