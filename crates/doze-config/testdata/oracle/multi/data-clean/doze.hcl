data_dir = "./d//x/"
service "s" {
  run = "x"
}
output "o" {
  value = service.s.data
}
