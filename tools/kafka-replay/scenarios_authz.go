package replay

import (
	"time"

	"github.com/twmb/franz-go/pkg/kmsg"
)

// authz-matrix (docs/port/debt.md I1): what Apache Kafka 4.3.1's StandardAuthorizer answers for each API family of the
// authorization matrix (behaviours/security.md SEC-60 to SEC-169), so the rules written from its source are held to a
// recording. bob has no ACL at all: every request he makes is refused or filtered, in the shape each API refuses in.
// alice may READ topic t and group g and DESCRIBE transactional id x: the permits, and the rights READ does not imply.
// The admin (a super user) makes the fixture and asks the two configs I9 is about.

// SEC-D54: Kafka sends nothing back for a DescribeUserScramCredentials with a null user list from a caller who may not
// DESCRIBE the cluster (its error answer iterates the null list and throws); this broker answers 31.
const limSCRAMNullList = "A DescribeUserScramCredentials with a null user list from a refused caller is answered 31."

var knownAuthzMatrix = []Known{
	{Step: "bob-describe-scram-all", Limitation: limSCRAMNullList},
}

func authzMatrix(e *Env) {
	authzFixture(e)
	fresh := func(name, user string) { e.Open(name, user, saslUsers[user]); e.Use(name) }

	fresh("bob", "bob")
	topicAPIs(e, "bob")
	groupAPIs(e, "bob")
	txnAPIs(e, "bob")
	credentialAPIs(e, "bob")
	aclAPIs(e, "bob")

	fresh("alice", "alice")
	topicAPIs(e, "alice")
	groupAPIs(e, "alice")
	txnAPIs(e, "alice")

	// I9: real Kafka's broker configs, asked by name, as a super user.
	e.Use("admin")
	d := kmsg.NewPtrDescribeConfigsRequest()
	r := kmsg.NewDescribeConfigsRequestResource()
	r.ResourceType, r.ResourceName = kmsg.ConfigResourceTypeBroker, "1"
	r.ConfigNames = []string{"allow.everyone.if.no.acl.found", "super.users", "authorizer.class.name"}
	d.Resources = append(d.Resources, r)
	e.Do("admin-broker-authorizer-configs", d, 4)
}

func authzFixture(e *Env) {
	e.Open("admin", "admin", saslUsers["admin"])
	e.Use("admin")
	e.Do("fixture-topics", createTopics(false, newTopic("t", 1, 1)), 7)
	acls := kmsg.NewPtrCreateACLsRequest()
	allow := func(kind kmsg.ACLResourceType, name string, op kmsg.ACLOperation) {
		acls.Creations = append(acls.Creations, aclCreation(kind, name, kmsg.ACLResourcePatternTypeLiteral, "User:alice", op))
	}
	allow(kmsg.ACLResourceTypeTopic, "t", kmsg.ACLOperationRead)
	allow(kmsg.ACLResourceTypeGroup, "g", kmsg.ACLOperationRead)
	allow(kmsg.ACLResourceTypeTransactionalId, "x", kmsg.ACLOperationDescribe)
	e.Do("fixture-acls", acls, 3)
	e.Await("the ACLs are listed", describeACLs(kmsg.ACLResourceTypeAny, nil, kmsg.ACLResourcePatternTypeAny), 3,
		func(r kmsg.Response) bool { return len(r.(*kmsg.DescribeACLsResponse).Resources) == 3 })
}

func topicAPIs(e *Env, who string) {
	l := func(s string) string { return who + "-" + s }
	e.Do(l("create-topics"), createTopics(false, newTopic(who+"-new", 1, 1)), 7)
	dt := kmsg.NewPtrDeleteTopicsRequest()
	dtt := kmsg.NewDeleteTopicsRequestTopic()
	dtt.Topic = str("t")
	dt.Topics, dt.TimeoutMillis = []kmsg.DeleteTopicsRequestTopic{dtt}, 15000
	e.Do(l("delete-topics"), dt, 6)
	e.Do(l("produce"), produceOne("t", 1), 9)
	f := kmsg.NewPtrFetchRequest()
	f.MaxWaitMillis, f.MaxBytes, f.ReplicaID, f.SessionEpoch = 0, 1<<20, -1, -1
	ft := kmsg.NewFetchRequestTopic()
	ft.Topic = "t"
	fp := kmsg.NewFetchRequestTopicPartition()
	fp.PartitionMaxBytes, fp.CurrentLeaderEpoch, fp.LogStartOffset = 1<<20, -1, -1
	ft.Partitions = append(ft.Partitions, fp)
	f.Topics = append(f.Topics, ft)
	e.Do(l("fetch"), f, 12)
	lo := kmsg.NewPtrListOffsetsRequest()
	lo.ReplicaID = -1
	lot := kmsg.NewListOffsetsRequestTopic()
	lot.Topic = "t"
	lop := kmsg.NewListOffsetsRequestTopicPartition()
	lop.Timestamp, lop.CurrentLeaderEpoch = -1, -1
	lot.Partitions = append(lot.Partitions, lop)
	lo.Topics = append(lo.Topics, lot)
	e.Do(l("list-offsets"), lo, 7)
	dr := kmsg.NewPtrDeleteRecordsRequest()
	drt := kmsg.NewDeleteRecordsRequestTopic()
	drt.Topic = "t"
	drp := kmsg.NewDeleteRecordsRequestTopicPartition()
	drp.Offset = 0
	drt.Partitions = append(drt.Partitions, drp)
	dr.Topics, dr.TimeoutMillis = append(dr.Topics, drt), 15000
	e.Do(l("delete-records"), dr, 2)
	cp := kmsg.NewPtrCreatePartitionsRequest()
	cpt := kmsg.NewCreatePartitionsRequestTopic()
	cpt.Topic, cpt.Count = "t", 2
	cp.Topics, cp.TimeoutMillis, cp.ValidateOnly = append(cp.Topics, cpt), 15000, true
	e.Do(l("create-partitions"), cp, 3)
	e.Do(l("describe-configs-topic"), describeTopicConfig("t", "retention.ms"), 4)
	dc := kmsg.NewPtrDescribeConfigsRequest()
	dcr := kmsg.NewDescribeConfigsRequestResource()
	dcr.ResourceType, dcr.ResourceName, dcr.ConfigNames = kmsg.ConfigResourceTypeBroker, "1", []string{"log.retention.ms"}
	dc.Resources = append(dc.Resources, dcr)
	e.Do(l("describe-configs-broker"), dc, 4)
	ac := kmsg.NewPtrAlterConfigsRequest()
	acr := kmsg.NewAlterConfigsRequestResource()
	acr.ResourceType, acr.ResourceName = kmsg.ConfigResourceTypeTopic, "t"
	acc := kmsg.NewAlterConfigsRequestResourceConfig()
	acc.Name, acc.Value = "retention.ms", str("100000")
	acr.Configs = append(acr.Configs, acc)
	ac.Resources, ac.ValidateOnly = append(ac.Resources, acr), true
	e.Do(l("alter-configs-topic"), ac, 2)
	inc := func(kind kmsg.ConfigResourceType, name, config string) *kmsg.IncrementalAlterConfigsRequest {
		r := kmsg.NewPtrIncrementalAlterConfigsRequest()
		res := kmsg.NewIncrementalAlterConfigsRequestResource()
		res.ResourceType, res.ResourceName = kind, name
		c := kmsg.NewIncrementalAlterConfigsRequestResourceConfig()
		c.Name, c.Op, c.Value = config, kmsg.IncrementalAlterConfigOpSet, str("100000")
		res.Configs = append(res.Configs, c)
		r.Resources, r.ValidateOnly = append(r.Resources, res), true
		return r
	}
	e.Do(l("incremental-alter-configs-topic"), inc(kmsg.ConfigResourceTypeTopic, "t", "retention.ms"), 1)
	e.Do(l("incremental-alter-configs-broker"), inc(kmsg.ConfigResourceTypeBroker, "1", "log.retention.ms"), 1)
	dp := kmsg.NewPtrDescribeProducersRequest()
	dpt := kmsg.NewDescribeProducersRequestTopic()
	dpt.Topic, dpt.Partitions = "t", []int32{0}
	dp.Topics = append(dp.Topics, dpt)
	e.Do(l("describe-producers"), dp, 0)
	md := metadata(false, "t")
	md.IncludeTopicAuthorizedOperations = true
	e.Do(l("metadata-authorized-operations"), md, 12)
}

func groupAPIs(e *Env, who string) {
	l := func(s string) string { return who + "-" + s }
	fc := kmsg.NewPtrFindCoordinatorRequest()
	fc.CoordinatorKey = "g"
	e.Do(l("find-group-coordinator-v3"), fc, 3)
	fc4 := kmsg.NewPtrFindCoordinatorRequest()
	fc4.CoordinatorKeys = []string{"g", "other"}
	e.Do(l("find-group-coordinator-v4"), fc4, 4)
	group := "other"
	if who == "bob" {
		group = "g"
	}
	j := kmsg.NewPtrJoinGroupRequest()
	j.Group, j.SessionTimeoutMillis, j.RebalanceTimeoutMillis, j.ProtocolType = group, 10000, 10000, "consumer"
	jp := kmsg.NewJoinGroupRequestProtocol()
	jp.Name, jp.Metadata = "range", []byte{0, 1, 0, 0, 0, 0, 0, 0, 0, 0}
	j.Protocols = append(j.Protocols, jp)
	e.Do(l("join-group"), j, 9)
	s := kmsg.NewPtrSyncGroupRequest()
	s.Group, s.Generation, s.MemberID = group, 1, "dz-member-1"
	s.ProtocolType, s.Protocol = str("consumer"), str("range")
	e.Do(l("sync-group"), s, 5)
	h := kmsg.NewPtrHeartbeatRequest()
	h.Group, h.Generation, h.MemberID = group, 1, "dz-member-1"
	e.Do(l("heartbeat"), h, 4)
	lg := kmsg.NewPtrLeaveGroupRequest()
	lg.Group = group
	lm := kmsg.NewLeaveGroupRequestMember()
	lm.MemberID = "dz-member-1"
	lg.Members = append(lg.Members, lm)
	e.Do(l("leave-group"), lg, 5)
	e.Do(l("offset-commit"), commitRequestFor(group, "t", 1), 9)
	of := kmsg.NewPtrOffsetFetchRequest()
	ofg := kmsg.NewOffsetFetchRequestGroup()
	ofg.Group = group
	of.Groups = append(of.Groups, ofg)
	e.Do(l("offset-fetch"), of, 8)
	e.Do(l("describe-groups"), describeGroupsRequest(group), 5)
	e.Do(l("list-groups"), kmsg.NewPtrListGroupsRequest(), 4)
	e.Do(l("delete-groups"), deleteGroupRequest(group), 2)
	od := kmsg.NewPtrOffsetDeleteRequest()
	od.Group = group
	odt := kmsg.NewOffsetDeleteRequestTopic()
	odt.Topic = "t"
	odp := kmsg.NewOffsetDeleteRequestTopicPartition()
	odt.Partitions = append(odt.Partitions, odp)
	od.Topics = append(od.Topics, odt)
	e.Do(l("offset-delete"), od, 0)
	cg := kmsg.NewPtrConsumerGroupHeartbeatRequest()
	cg.Group, cg.MemberID, cg.MemberEpoch, cg.RebalanceTimeoutMillis = group, "m-"+who+"-0000000000", 0, 10000
	cg.SubscribedTopicNames = []string{"t"}
	cg.Topics = []kmsg.ConsumerGroupHeartbeatRequestTopic{}
	e.Do(l("consumer-group-heartbeat"), cg, 1)
	cd := kmsg.NewPtrConsumerGroupDescribeRequest()
	cd.Groups = []string{group}
	e.Do(l("consumer-group-describe"), cd, 1)
}

func txnAPIs(e *Env, who string) {
	l := func(s string) string { return who + "-" + s }
	fc := kmsg.NewPtrFindCoordinatorRequest()
	fc.CoordinatorType, fc.CoordinatorKeys = 1, []string{"x"}
	e.Do(l("find-txn-coordinator"), fc, 4)
	ip := kmsg.NewPtrInitProducerIDRequest()
	ip.TransactionalID, ip.TransactionTimeoutMillis, ip.ProducerID, ip.ProducerEpoch = str("x"), 60000, -1, -1
	e.Do(l("init-producer-id-transactional"), ip, 5)
	ap := kmsg.NewPtrAddPartitionsToTxnRequest()
	ap.TransactionalID, ap.ProducerID, ap.ProducerEpoch = "x", 1000, 0
	apt := kmsg.NewAddPartitionsToTxnRequestTopic()
	apt.Topic, apt.Partitions = "t", []int32{0}
	ap.Topics = append(ap.Topics, apt)
	e.Do(l("add-partitions-to-txn"), ap, 3)
	ao := kmsg.NewPtrAddOffsetsToTxnRequest()
	ao.TransactionalID, ao.ProducerID, ao.ProducerEpoch, ao.Group = "x", 1000, 0, "g"
	e.Do(l("add-offsets-to-txn"), ao, 3)
	tc := kmsg.NewPtrTxnOffsetCommitRequest()
	tc.TransactionalID, tc.Group, tc.ProducerID, tc.ProducerEpoch, tc.Generation, tc.MemberID = "x", "g", 1000, 0, -1, ""
	tct := kmsg.NewTxnOffsetCommitRequestTopic()
	tct.Topic = "t"
	tcp := kmsg.NewTxnOffsetCommitRequestTopicPartition()
	tcp.Offset, tcp.LeaderEpoch = 1, -1
	tct.Partitions = append(tct.Partitions, tcp)
	tc.Topics = append(tc.Topics, tct)
	e.Do(l("txn-offset-commit"), tc, 3)
	et := kmsg.NewPtrEndTxnRequest()
	et.TransactionalID, et.ProducerID, et.ProducerEpoch, et.Commit = "x", 1000, 0, true
	e.Do(l("end-txn"), et, 3)
	dt := kmsg.NewPtrDescribeTransactionsRequest()
	dt.TransactionalIDs = []string{"x", "y"}
	e.Do(l("describe-transactions"), dt, 0)
	e.Do(l("list-transactions"), kmsg.NewPtrListTransactionsRequest(), 1)
}

func credentialAPIs(e *Env, who string) {
	l := func(s string) string { return who + "-" + s }
	// SEC-D54: a null list from a caller without DESCRIBE on the cluster.
	e.DoOrSilence(l("describe-scram-all"), kmsg.NewPtrDescribeUserSCRAMCredentialsRequest(), 0, 5*time.Second)
	d := kmsg.NewPtrDescribeUserSCRAMCredentialsRequest()
	u := kmsg.NewDescribeUserSCRAMCredentialsRequestUser()
	u.Name = "alice"
	d.Users = append(d.Users, u)
	e.Do(l("describe-scram-alice"), d, 0)
	a := kmsg.NewPtrAlterUserSCRAMCredentialsRequest()
	del := kmsg.NewAlterUserSCRAMCredentialsRequestDeletion()
	del.Name, del.Mechanism = "alice", 1
	a.Deletions = append(a.Deletions, del)
	e.Do(l("alter-scram-delete-alice"), a, 0)
}

func aclAPIs(e *Env, who string) {
	l := func(s string) string { return who + "-" + s }
	c := kmsg.NewPtrCreateACLsRequest()
	c.Creations = append(c.Creations, aclCreation(kmsg.ACLResourceTypeTopic, "t", kmsg.ACLResourcePatternTypeLiteral, "User:"+who, kmsg.ACLOperationAll))
	e.Do(l("create-acls"), c, 3)
	e.Do(l("describe-acls"), describeACLs(kmsg.ACLResourceTypeAny, nil, kmsg.ACLResourcePatternTypeAny), 3)
	d := kmsg.NewPtrDeleteACLsRequest()
	f := kmsg.NewDeleteACLsRequestFilter()
	f.ResourceType, f.ResourcePatternType = kmsg.ACLResourceTypeAny, kmsg.ACLResourcePatternTypeAny
	f.Operation, f.PermissionType = kmsg.ACLOperationAny, kmsg.ACLPermissionTypeAny
	d.Filters = append(d.Filters, f)
	e.Do(l("delete-acls"), d, 3)
}
