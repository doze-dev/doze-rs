module fake "a" { version = 1 }
app "w" { run = "true" }
module fake "b" {
  version    = 1
  depends_on = { "module.a" = "started", "w" = "healthy" }
}
