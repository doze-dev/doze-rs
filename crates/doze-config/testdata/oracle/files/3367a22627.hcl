
app "api" {
  run  = "x"
  port = 8080
}
app "web" {
  run = "y"
  env = { API_URL = app.api.url }
}
