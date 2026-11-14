app "s" {
  run = "x"
  env = { A = null }
}
task "t" {
  run    = "x"
  manual = true
  inputs = ["a", null]
}
