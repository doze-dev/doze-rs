stack = "demo"
defaults { names = true }
app "api" {
  run  = "sleep 1"
  port = 28080
}
app "raw" {
  run    = "sleep 1"
  port   = 28081
  direct = true
}
