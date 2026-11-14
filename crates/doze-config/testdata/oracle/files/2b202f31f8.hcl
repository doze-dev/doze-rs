
stack = "watch"

defaults {
  names = false
}

task "gen" {
  run    = "echo gen >> gen.log"
  inputs = ["schema/*.sql"]
}

app "api" {
  run   = "echo started >> starts.log; sleep 600"
  watch = ["src/*.txt"]
  after = [task.gen]
}
