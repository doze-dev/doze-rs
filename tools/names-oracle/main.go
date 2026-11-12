// The Go doze-names library's own answers, for the Rust crate to match (docs/port/names/PLAN.md N0): the address
// each qualified name gets (the FNV-1a hash and probe must agree, or a Go and a Rust program would put one name at
// two addresses), and a registry file written by the library (the Rust crate must read it and write one Go reads).
//
//	go run . addresses > ../../crates/doze-names/tests/fixtures/addresses.txt
//	go run . registry <home>        writes <home>/names.json with the library, prints its pid, holds until stdin closes
//	go run . read <home>            prints what the library resolves from <home>/names.json
package main

import (
	"fmt"
	"io"
	"os"

	names "github.com/doze-dev/doze-names"
)

func main() {
	switch os.Args[1] {
	case "addresses":
		// A fresh registry per name: no collision shifts, only the hash.
		for _, svc := range []string{"db", "cache", "api", "web", "aws", "kafka", "worker", "cloud", "temporal", "a", "zz-top", "svc-299"} {
			for _, stack := range []string{"shop", "blog", "harbour"} {
				dir, _ := os.MkdirTemp("", "names-oracle")
				l, err := names.Open(dir, "doze").Claim(names.Qualified(svc, stack))
				if err != nil {
					panic(err)
				}
				fmt.Printf("%s %s\n", l.Name.Host, l.IP)
				os.RemoveAll(dir)
			}
		}
	case "registry":
		r := names.Open(os.Args[2], "doze-aws")
		if _, err := r.Claim(names.Apex("aws")); err != nil {
			panic(err)
		}
		l, err := r.Claim(names.Qualified("aws", "harbour"))
		if err != nil {
			panic(err)
		}
		if err := l.Route("127.0.0.4:4566"); err != nil {
			panic(err)
		}
		fmt.Println(os.Getpid())
		// Stay alive (the names are pruned once this process is gone) until the reader closes stdin.
		_, _ = io.Copy(io.Discard, os.Stdin)
	case "read":
		r := names.Open(os.Args[2], "doze")
		for _, h := range []string{"aws.doze", "aws.harbour.doze", "sqs.ap-south-1.aws.harbour.doze", "kafka.doze", "db.shop.doze"} {
			fmt.Printf("%s %v\n", h, r.Resolve(h))
		}
	}
}
