# map-oracle

The Go test that produced `crates/doze-map/tests/data/go-flow.json`: Go's `internal/desk/view` (flow.go) laying out the
Larder and shop fixtures and 200 generated stacks. It must run inside the Go package, so copy it into a doze checkout:

    cp tools/map-oracle/zz_oracle_test.go ../doze/internal/desk/view/
    (cd ../doze && go test ./internal/desk/view -run Oracle)   # writes the JSON; see the file for the output path
    rm ../doze/internal/desk/view/zz_oracle_test.go
