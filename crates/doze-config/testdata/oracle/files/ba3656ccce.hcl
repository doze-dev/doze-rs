
module fake "apple" { version = 1 }
module fake "b" {
  version    = 1
  depends_on = { "module.appel" = "healthy" }
}
