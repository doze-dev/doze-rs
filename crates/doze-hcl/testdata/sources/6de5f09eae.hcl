variable "used" { default = "x" }
variable "spare" { default = "y" }
task "migrate" { run = "m" }
task "seed" { run = "s" }
task "once" {
  run    = "o"
  manual = true
}
app "api" {
  run   = "a ${var.used}"
  after = [task.migrate]
}
