
module nosuchengine "jobs" {
  made_up = true
}
app "worker" {
  run = "x"
  env = {
    QUEUE = module.jobs.arn
    PORT  = module.jobs.port
  }
}
