app "w" {
  run = "x"
  env = { Y = app.w.port }
}
