stack = "larder"                          # root fields
defaults { names = true }

module postgres "db" { version = 18 }     # an engine from the doze registry
service "mailpit" { run = "mailpit" }     # anything else
task "migrate" { run = "bin/migrate up" } # runs to completion
app "api" { run = "go run ./cmd/api" }    # your code
