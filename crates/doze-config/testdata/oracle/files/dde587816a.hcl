
stack = "svc"

defaults {
  names = false
}

service "termy" {
  run  = "trap 'touch $DATA/got-term; exit 0' TERM; while :; do sleep 0.2; done"
}

service "polite" {
  run  = "trap '' TERM INT; while :; do sleep 0.2; done"
  stop = ["touch $DATA/stop-ran; pkill -KILL -f 'while :; do sleep 0.2; done' || true"]
}
