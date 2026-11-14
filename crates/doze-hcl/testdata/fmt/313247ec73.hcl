app "api" {
  run   = "x"
  port  = 8080
  after = [task.migrate]
}
task "migrate" {
  run = "y"
  env = { API = app.api.url }
}
