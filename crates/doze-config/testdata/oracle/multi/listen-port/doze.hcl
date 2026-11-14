module fake "p" {
  version = 1
  listen  = "127.0.0.1:99999999999999999999"
}
output "o" {
  value = module.p.port
}
