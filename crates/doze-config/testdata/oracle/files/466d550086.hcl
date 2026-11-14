
defaults {
  ready_timeout = "3m"
  env           = { LOG = "debug", REGION = "eu" }
  restart {
    policy = "always"
  }
}
app "api" {
  run  = "x"
  port = 3000
  env  = { REGION = "us" }
}
service "mail" {
  run           = "y"
  ready_timeout = "10s"
}
task "seed" { run = "z" }
app "w" {
  run = "w"
  after = [task.seed]
}
