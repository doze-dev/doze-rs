
module fake "a" { version = 1 }
output "secret" {
  value     = module.a.name
  sensitive = true
}
