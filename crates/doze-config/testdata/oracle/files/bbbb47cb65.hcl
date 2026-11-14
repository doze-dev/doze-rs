app "w" { run = "x" }
app "api" {
  run = "x"
  env = { U = app.w.url }
}
