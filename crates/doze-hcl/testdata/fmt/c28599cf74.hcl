stack = "embedtest"

app "ticker" {
  run = "sh -c 'while true; do echo tick; sleep 1; done'"
}
