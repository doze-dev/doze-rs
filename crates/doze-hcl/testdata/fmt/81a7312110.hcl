stack = "noisy"

app "quiet" {
  run = "sleep 600"
}

app "noisy" {
  run = "seq 1 100000 | sed 's/$/ a line of ordinary length, the kind a web server prints for each request/'; sleep 600"
}
