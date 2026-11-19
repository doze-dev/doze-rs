stack = "tasks"

defaults {
  names = false
}

task "migrate" {
  run = "echo 'no such table' >&2; test -f ok"
}

task "seed" {
  run    = "echo seeded >> seeded.log"
  manual = true
}

app "api" {
  run   = "sleep 600"
  after = [task.migrate]
}
