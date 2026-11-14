// The Go kafka module's oracle (docs/port/phase11.md): runs doze-kafka/module in-process on a corpus of blocks and
// records its answers, which the Rust module face of doze-kafka must match or differ from only as listed. A local tool:
// it needs ../../../../doze-kafka checked out.
module doze-rs/tools/module-oracle/kafka

go 1.27.0

require (
	github.com/doze-dev/doze-kafka/module v0.0.0
	github.com/doze-dev/doze-sdk v0.12.0
	github.com/hashicorp/hcl/v2 v2.25.0
)

require (
	github.com/BurntSushi/toml v1.6.0 // indirect
	github.com/agext/levenshtein v1.2.1 // indirect
	github.com/apparentlymart/go-textseg/v15 v15.0.0 // indirect
	github.com/apparentlymart/go-textseg/v17 v17.0.1 // indirect
	github.com/bufbuild/protocompile v0.14.1 // indirect
	github.com/doze-dev/doze-kafka v1.2.0 // indirect
	github.com/go-viper/mapstructure/v2 v2.4.0 // indirect
	github.com/golang/snappy v1.0.0 // indirect
	github.com/google/go-cmp v0.7.0 // indirect
	github.com/hamba/avro/v2 v2.31.0 // indirect
	github.com/json-iterator/go v1.1.12 // indirect
	github.com/klauspost/compress v1.20.2 // indirect
	github.com/mitchellh/go-wordwrap v1.0.1 // indirect
	github.com/modern-go/concurrent v0.0.0-20180306012644-bacd9c7ef1dd // indirect
	github.com/modern-go/reflect2 v1.0.2 // indirect
	github.com/pierrec/lz4/v4 v4.1.33 // indirect
	github.com/santhosh-tekuri/jsonschema/v6 v6.0.3 // indirect
	github.com/twmb/franz-go v1.22.1 // indirect
	github.com/twmb/franz-go/pkg/kadm v1.19.0 // indirect
	github.com/twmb/franz-go/pkg/kmsg v1.14.0 // indirect
	github.com/zclconf/go-cty v1.19.0 // indirect
	go.etcd.io/bbolt v1.5.0 // indirect
	golang.org/x/sync v0.23.0 // indirect
	golang.org/x/sys v0.48.0 // indirect
	golang.org/x/text v0.42.0 // indirect
	google.golang.org/protobuf v1.36.12 // indirect
)

replace (
	github.com/doze-dev/doze-kafka => ../../../../doze-kafka
	github.com/doze-dev/doze-kafka/module => ../../../../doze-kafka/module
)
