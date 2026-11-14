
app "worker" {
  run   = "x"
  after = [app.api]
}
app "api" {
  run = "y"
}
