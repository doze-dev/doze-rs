
stack = "tasks"

defaults {
  names = false
}

task "migrate" {
  run    = "echo ran >> ran.log"
  inputs = ["migrations/*.sql"]
}

app "api" {
  run   = "sleep 600"
  after = [task.migrate]
}
