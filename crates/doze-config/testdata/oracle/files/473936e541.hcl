
app "api" { run = "x" }
app "web" {
  run = "y"
  env = { API = process.api.url }
}
