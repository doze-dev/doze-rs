
stack = "dl"

defaults {
  names = false
}

service "tool" {
  download = "%s/tool.tgz"
  run      = "bin/serve"
  port     = %d
}
