// The Go module oracle (docs/port/modules/PLAN.md step 5): runs the Go modules in-process on a corpus of blocks and
// records their answers, which the Rust modules must match. A local tool: it needs ../../../doze-modules checked out.
module doze-rs/tools/module-oracle

go 1.27.0

require (
	github.com/doze-dev/doze-modules v0.0.0
	github.com/doze-dev/doze-sdk v0.11.0
	github.com/hashicorp/hcl/v2 v2.24.0
	github.com/zclconf/go-cty v1.18.1
)

require (
	github.com/agext/levenshtein v1.2.1 // indirect
	github.com/apparentlymart/go-textseg/v15 v15.0.0 // indirect
	github.com/golang/snappy v0.0.4 // indirect
	github.com/google/go-cmp v0.7.0 // indirect
	github.com/klauspost/compress v1.18.7 // indirect
	github.com/mitchellh/go-wordwrap v1.0.1 // indirect
	github.com/montanaflynn/stats v0.7.1 // indirect
	github.com/xdg-go/pbkdf2 v1.0.0 // indirect
	github.com/xdg-go/scram v1.1.2 // indirect
	github.com/xdg-go/stringprep v1.0.4 // indirect
	github.com/youmark/pkcs8 v0.0.0-20240726163527-a2c0da244d78 // indirect
	go.mongodb.org/mongo-driver v1.17.9 // indirect
	golang.org/x/crypto v0.57.0 // indirect
	golang.org/x/sync v0.23.0 // indirect
	golang.org/x/text v0.42.0 // indirect
)

replace github.com/doze-dev/doze-modules => ../../../doze-modules
