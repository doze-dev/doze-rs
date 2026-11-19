package replay

import (
	"doze-rs/tools/kafka-replay/config"
	"github.com/twmb/franz-go/pkg/kmsg"
)

// Scenarios recorded to clear the broker's debt (docs/port/debt.md, 2026-11-18):
//
//   - sasl-handshake-v0: SaslHandshake v0 and the raw tokens after it (SEC-D3, I14), against Apache Kafka 4.3.1,
//     which still serves v0 (every recorded broker lists SaslHandshake 0-1).
//   - acl-v0: the ACL APIs at v0 (I2), against Apache Kafka 3.9.1, the last release that serves them; replayed
//     against --kafka-version 3.
//   - authz-matrix: a refusal (and some permits) for every API family of the authorization matrix (I1, I17, I9,
//     I10; SEC-D20, SEC-D34 to D36, SEC-D54), against Apache Kafka 4.3.1.

func init() {
	authz := func(name string, run func(*Env), known []Known) Scenario {
		return Scenario{Name: name, Run: run, Users: saslUsers, Admin: "admin", Known: known,
			KafkaEnv: []string{
				"KAFKA_AUTHORIZER_CLASS_NAME=org.apache.kafka.metadata.authorizer.StandardAuthorizer",
				"KAFKA_SUPER_USERS=User:admin;User:ANONYMOUS",
			},
			Doze: func(c *config.Config) { c.ACLDefault, c.ACLSuperUsers = "deny", []string{"User:admin"} }}
	}
	Scenarios = append(Scenarios,
		authz("sasl-handshake-v0", saslHandshakeV0, knownHandshakeV0),
		Scenario{Name: "acl-v0", Run: aclV0, Image: "apache/kafka:3.9.1", KafkaVersion: 3,
			KafkaEnv: []string{
				"KAFKA_AUTHORIZER_CLASS_NAME=org.apache.kafka.metadata.authorizer.StandardAuthorizer",
				"KAFKA_SUPER_USERS=User:ANONYMOUS",
			},
			Doze: func(c *config.Config) { c.ACLDefault, c.ACLSuperUsers = "deny", []string{"User:ANONYMOUS"} }},
		authz("authz-matrix", authzMatrix, knownAuthzMatrix),
	)
}

var knownHandshakeV0 = []Known{
	{Step: "v0-versions-first-versions", Path: "ApiKeys", Limitation: limAPIList},
}

// --- SaslHandshake v0 ---

func saslHandshakeV0(e *Env) {
	saslFixture(e)
	v0 := func(label, mechanism string) {
		h := kmsg.NewPtrSASLHandshakeRequest()
		h.Mechanism = mechanism
		e.Do(label, h, 0)
	}
	// A right PLAIN token, raw: Kafka writes PLAIN's empty reply as a frame of length 0, and the connection is alice's.
	fresh(e, "v0-good")
	v0("v0-good-handshake", "PLAIN")
	e.Raw("v0-good-token", []byte("\x00alice\x00alice-secret"))
	whoAmI(e, "v0-good-who")
	// bob, with ApiVersions first, as an old client opens.
	fresh(e, "v0-versions-first")
	apiVersions(e, "v0-versions-first-versions")
	v0("v0-versions-first-handshake", "PLAIN")
	e.Raw("v0-versions-first-token", []byte("\x00bob\x00bob-secret"))
	whoAmI(e, "v0-versions-first-who")
	// Refusals: raw framing has no error to send, so the connection closes.
	for _, c := range []struct{ name, token string }{
		{"v0-wrong-password", "\x00alice\x00nope"},
		{"v0-authzid-other-user", "bob\x00alice\x00alice-secret"},
		{"v0-not-a-token", "not a plain token"},
		{"v0-empty", ""},
	} {
		fresh(e, c.name)
		v0(c.name+"-handshake", "PLAIN")
		e.Raw(c.name+"-token", []byte(c.token))
	}
	// A mechanism that is not offered: answered with the list, then closed.
	fresh(e, "v0-unsupported")
	v0("v0-unsupported-handshake", "SCRAM-SHA-256")
	// A Kafka request after a v0 handshake is read as a raw token, and is not a PLAIN one.
	fresh(e, "v0-framed")
	v0("v0-framed-handshake", "PLAIN")
	authenticate(e, "v0-framed-authenticate", []byte("\x00alice\x00alice-secret"), 1)
}

// --- the ACL APIs at v0 (Kafka 3.9.1) ---

func aclCreation(kind kmsg.ACLResourceType, name string, pattern kmsg.ACLResourcePatternType, principal string, op kmsg.ACLOperation) kmsg.CreateACLsRequestCreation {
	c := kmsg.NewCreateACLsRequestCreation()
	c.ResourceType, c.ResourceName, c.ResourcePatternType = kind, name, pattern
	c.Principal, c.Host, c.Operation, c.PermissionType = principal, "*", op, kmsg.ACLPermissionTypeAllow
	return c
}

func describeACLs(kind kmsg.ACLResourceType, name *string, pattern kmsg.ACLResourcePatternType) *kmsg.DescribeACLsRequest {
	d := kmsg.NewPtrDescribeACLsRequest()
	d.ResourceType, d.ResourceName, d.ResourcePatternType = kind, name, pattern
	d.Operation, d.PermissionType = kmsg.ACLOperationAny, kmsg.ACLPermissionTypeAny
	return d
}

func aclV0(e *Env) {
	create := kmsg.NewPtrCreateACLsRequest()
	create.Creations = []kmsg.CreateACLsRequestCreation{
		aclCreation(kmsg.ACLResourceTypeTopic, "orders", kmsg.ACLResourcePatternTypeLiteral, "User:alice", kmsg.ACLOperationRead),
		aclCreation(kmsg.ACLResourceTypeTopic, "orders", kmsg.ACLResourcePatternTypeLiteral, "User:bob", kmsg.ACLOperationWrite),
	}
	e.Do("create-v0", create, 0)
	prefixed := kmsg.NewPtrCreateACLsRequest()
	prefixed.Creations = []kmsg.CreateACLsRequestCreation{
		aclCreation(kmsg.ACLResourceTypeTopic, "ord", kmsg.ACLResourcePatternTypePrefixed, "User:alice", kmsg.ACLOperationWrite),
	}
	e.Do("create-v1-prefixed", prefixed, 1)
	e.Await("the three ACLs are listed", describeACLs(kmsg.ACLResourceTypeTopic, nil, kmsg.ACLResourcePatternTypeAny), 1,
		func(r kmsg.Response) bool {
			n := 0
			for _, res := range r.(*kmsg.DescribeACLsResponse).Resources {
				n += len(res.ACLs)
			}
			return n == 3
		})
	// v0 has no pattern type: its filter is LITERAL, so the PREFIXED ACL is not listed.
	e.Do("describe-v0-topics", describeACLs(kmsg.ACLResourceTypeTopic, nil, kmsg.ACLResourcePatternTypeLiteral), 0)
	e.Do("describe-v0-by-name", describeACLs(kmsg.ACLResourceTypeTopic, str("orders"), kmsg.ACLResourcePatternTypeLiteral), 0)
	e.Do("describe-v0-unknown-name", describeACLs(kmsg.ACLResourceTypeTopic, str("nothing"), kmsg.ACLResourcePatternTypeLiteral), 0)
	e.Do("describe-v1-prefixed", describeACLs(kmsg.ACLResourceTypeTopic, nil, kmsg.ACLResourcePatternTypePrefixed), 1)
	del := func(name string) *kmsg.DeleteACLsRequest {
		d := kmsg.NewPtrDeleteACLsRequest()
		f := kmsg.NewDeleteACLsRequestFilter()
		f.ResourceType, f.ResourceName, f.ResourcePatternType = kmsg.ACLResourceTypeTopic, str(name), kmsg.ACLResourcePatternTypeLiteral
		f.Operation, f.PermissionType = kmsg.ACLOperationRead, kmsg.ACLPermissionTypeAny
		d.Filters = append(d.Filters, f)
		return d
	}
	e.Do("delete-v0-read", del("orders"), 0)
	e.Await("alice's read is gone", describeACLs(kmsg.ACLResourceTypeTopic, str("orders"), kmsg.ACLResourcePatternTypeLiteral), 0,
		func(r kmsg.Response) bool {
			res := r.(*kmsg.DescribeACLsResponse).Resources
			return len(res) == 1 && len(res[0].ACLs) == 1
		})
	e.Do("delete-v0-read-again", del("orders"), 0)
	e.Do("describe-v0-after", describeACLs(kmsg.ACLResourceTypeTopic, nil, kmsg.ACLResourcePatternTypeLiteral), 0)
}
