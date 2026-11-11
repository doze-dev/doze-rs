> Research record, 2026-11-12, kept as evidence for [`../phase8.md`](../phase8.md). The Kafka 4.3.1 source files it cites were fetched to a scratch directory that will not last: re-fetch from `apache/kafka` tag `4.3.1` (commit 26b251a) to follow a line number. Where a statement says "recorded" it means the Go repo's recordings of real Kafka 4.3.1.

# Kafka 4.3.1 security semantics, for the doze-kafka Rust port

Source of every Kafka fact below: `apache/kafka` tag `4.3.1` (annotated tag object `a07059eb...`, commit
`26b251a451ce941d3d7a55e6487bcb7f16b5ad48`, "Bump version to 4.3.1"). A sparse checkout of the files read is at
`/private/tmp/claude-501/-Users-srini-Code-doze-dev/014d8981-fa59-41b3-a494-fc4a4035c0bb/scratchpad/phase8/k431/`;
line numbers are absolute lines of those files. `QuorumController.java`, `ReplicationControlManager.java` and
`ServerConfigs.java` were fetched whole into `.../scratchpad/phase8/` (same tag).

Path abbreviations (all relative to the Kafka repo root):

| Abbrev | Path |
|---|---|
| KA | `core/src/main/scala/kafka/server/KafkaApis.scala` |
| CA | `core/src/main/scala/kafka/server/ControllerApis.scala` |
| AH | `core/src/main/scala/kafka/server/AuthHelper.scala` |
| AclApis | `core/src/main/scala/kafka/server/AclApis.scala` |
| CH | `core/src/main/scala/kafka/server/ConfigHelper.scala` |
| CAM | `core/src/main/scala/kafka/server/ConfigAdminManager.scala` |
| SS | `core/src/main/scala/kafka/network/SocketServer.scala` |
| RHH | `core/src/main/scala/kafka/server/RequestHandlerHelper.scala` |
| SAD | `metadata/src/main/java/org/apache/kafka/metadata/authorizer/StandardAuthorizerData.java` |
| SA | `metadata/src/main/java/org/apache/kafka/metadata/authorizer/StandardAuthorizer.java` |
| StdAcl | `metadata/src/main/java/org/apache/kafka/metadata/authorizer/StandardAcl.java` |
| ACM | `metadata/src/main/java/org/apache/kafka/controller/AclControlManager.java` |
| SCM | `metadata/src/main/java/org/apache/kafka/controller/ScramControlManager.java` |
| Authorizer | `clients/src/main/java/org/apache/kafka/server/authorizer/Authorizer.java` |
| AclEntry | `server/src/main/java/org/apache/kafka/security/authorizer/AclEntry.java` |
| SSA | `clients/src/main/java/org/apache/kafka/common/security/authenticator/SaslServerAuthenticator.java` |
| DKPB | `clients/src/main/java/org/apache/kafka/common/security/authenticator/DefaultKafkaPrincipalBuilder.java` |
| Plain | `clients/src/main/java/org/apache/kafka/common/security/plain/internals/PlainSaslServer.java` |
| Scram* | `clients/src/main/java/org/apache/kafka/common/security/scram/internals/Scram{SaslServer,Messages,Formatter,Mechanism}.java` |
| Sel | `clients/src/main/java/org/apache/kafka/common/network/Selector.java` |
| KC | `clients/src/main/java/org/apache/kafka/common/network/KafkaChannel.java` |
| Errors | `clients/src/main/java/org/apache/kafka/common/protocol/Errors.java` |
| msg | `clients/src/main/resources/common/message/*.json` |

Context that shapes everything: in KRaft a client request lands on a broker (`KafkaApis`). Some APIs are handled
there; others are forwarded to the controller (`forwardToController`, KA:86-87, 97-98, 104, 118, 120) and
authorized there (`ControllerApis`) against the **original client principal** carried in the envelope. doze-kafka is one
process, so the observable rule is: use the handler's authorization, wherever Kafka runs it. The matrix names which
file each row comes from.

"Recorded" below means: doze-kafka's Go repo holds recordings made against real `apache/kafka:4.3.1`
(`~/Code/doze-dev/doze-kafka/conformance/recordings/*.json`, claims in
`~/Code/doze-dev/doze-kafka/conformance/claims/auth.md` rows A1-A18, `api.md` V5/V8, `traffic.md` R2). Where
source reading and recording both exist they agree unless a "CORRECTION" says otherwise.

---

## 1. Authorizer semantics

### 1.1 Enablement and the no-authorizer case

- `authorizer.class.name` defaults to `""` (`server-common/src/main/java/org/apache/kafka/server/config/ServerConfigs.java:119-120`).
  Empty means **no authorizer at all** (`KafkaConfig.scala:253-260`): `AuthHelper.authorize` returns true for everything
  (`authorizer.forall`, AH:50); `filterByAuthorized` returns every resource (AH:118); `authorizedOperations` returns every
  supported operation (AH:72-73); `partitionSeqByAuthorized` returns everything authorized (AH:96).
- With no authorizer: DescribeAcls answers `SECURITY_DISABLED` (54) with message `No Authorizer is configured on the broker`
  (AclApis:66-72); CreateAcls and DeleteAcls answer every item with `SECURITY_DISABLED` via `getErrorResponse(new
  SecurityDisabledException("No Authorizer is configured."))` (AclApis:88-92, 143-148). The cluster-level ALTER/DESCRIBE
  check runs first (AclApis:63, 85, 141) but is trivially true here.
- The KRaft authorizer is `org.apache.kafka.metadata.authorizer.StandardAuthorizer` (config class name in
  `authorizer.class.name`); its two settings are `super.users` and `allow.everyone.if.no.acl.found`
  (SA:58-60). `acl-default = deny|allow` in the Go harness maps to the latter.
- `super.users`: split on `;`, each trimmed, empty pieces dropped, each must parse as `Type:name` (first `:` splits,
  `SecurityUtils.parseKafkaPrincipal`, `clients/.../utils/SecurityUtils.java:64-76`); a bad entry is a startup failure
  (SA:195-208). `allow.everyone.if.no.acl.found`: absent -> DENIED; else `Boolean.parseBoolean(trim)` (SA:210-214).
- Requests are not processed on a listener until the authorizer has loaded the ACL log (`earlyStartListeners` /
  `endpoint futures`, SA:125-137); until then non-super users would get `AuthorizerNotReadyException`
  (SAD:239-241). A single-process port with synchronous ACL state has no such window.

### 1.2 The principal and host that are matched

- Principal: the channel's `KafkaPrincipal` (type + name). `baseKafkaPrincipal` strips any subclass to plain
  `KafkaPrincipal(type,name)` (SAD:456-461). Equality is exact on type and name, case-sensitive
  (`KafkaPrincipal.java:67-74`); `tokenAuthenticated` does not take part.
- An ACL's principal string is split at the **first** `:` (`StandardAcl.kafkaPrincipal`, StdAcl:59-68). A principal
  matches an ACL if the ACL principal equals the session principal **or equals `User:*`** (SAD:463-469,
  `matchingPrincipals = {session principal, User:*}`; line 488 `contains`). The wildcard is exactly the string
  `User:*`: it matches any principal of any type (a `Group:x` session principal is matched by `User:*`), and `Group:*` is
  not a wildcard.
- Host: the ACL host is `*` (matches all) or must be **string-equal** to `clientAddress().getHostAddress()` of the
  TCP peer (SAD:492; `RequestContext.clientAddress`). No CIDR, no hostnames, no case folding, no validation at
  creation. IPv6 peers render as Java prints them (fully expanded, e.g. `0:0:0:0:0:0:0:1`, not `::1`) - an ACL
  created with `::1` never matches (Unverified by recording; derived from `InetAddress.getHostAddress`).
- Default principals: PLAINTEXT listener -> `User:ANONYMOUS`; SSL -> the peer DN or ANONYMOUS; SASL -> `User:<authorization id>`
  (see 3.9).

### 1.3 Matching algorithm (`StandardAuthorizerData.authorize`, SAD:226-250)

Only LITERAL resource patterns are authorized (`IllegalArgumentException` otherwise, SAD:230-232); the authorizer is
always called with `PatternType.LITERAL` and the concrete resource name (AH:51, 66, 110).

Order inside one `authorize(action)`:
1. If `super.users` contains `principal.toString()` -> ALLOWED, rule `SuperUser` (SAD:236-238). This is checked before
   anything else, so super users **bypass DENY ACLs** and the loading check.
2. Else if ACL loading is not complete -> `AuthorizerNotReadyException` (SAD:239-241).
3. Else `findAclRule` (SAD:310-364): scan, for the action's resource type, the ACLs whose resource name is a prefix of
   the resource name (PREFIXED ACLs) or equals it (LITERAL ACLs), then the LITERAL `*` ACLs of that type:
   - LITERAL ACL matches iff `aclName == resourceName` (a LITERAL ACL whose name is only a *prefix* is skipped,
     SAD:398-405); PREFIXED matches iff `resourceName.startsWith(aclName)` (equal names included); LITERAL `*`
     matches every name (SAD:406, 354-362). Resource types are separate namespaces.
   - **hasResourceAcls** is set the moment *any* ACL matches the resource name/prefix/wildcard, regardless of
     principal, host or operation (SAD:422).
   - For each such ACL `findResult` (SAD:483-523) decides: principal in the matching set? host `*` or equal? operation
     check (1.4)? If all yes -> the ACL's permission (ALLOWED/DENIED), else no result (null).
   - First DENY found in the specific-name section **stops the scan and wins** (SAD:424-429, 347-349); if none, the
     wildcard section is scanned; a DENY there still wins over an ALLOW from the specific section
     (`MatchingRuleBuilder.build`: denyAcl first, then allowAcl, SAD:584-594).
4. Result precedence (`MatchingRuleBuilder.build`, SAD:584-594):
   DENY match -> DENIED; else ALLOW match -> ALLOWED; else **if no ACL at all matched the resource** (`!hasResourceAcls`)
   -> the default rule (ALLOWED if `allow.everyone.if.no.acl.found=true`, else DENIED); else (resource has ACLs but none
   applied to this principal/host/op) -> DENIED regardless of `allow.everyone.if.no.acl.found`.

   So `allow.everyone.if.no.acl.found` is per *resource*, not global: one ACL for anybody on a topic (or a prefix or
   wildcard covering it) turns the default off for that topic.
5. The default with an authorizer and an empty ACL set is DENY for every principal except super users. This is why a
   freshly configured broker with `acl-default = "deny"` refuses everything (recorded: `telemetry-acl`,
   Metadata `Topics[0].ErrorCode: 29` for a user with no ACLs).

### 1.4 Operations: implication and DENY

`findResult` operation check (SAD:495-520). ACL operation `ALL` matches every action operation, for both ALLOW and DENY.
Otherwise:
- ALLOW ACL, action DESCRIBE: matches if ACL op is one of **DESCRIBE, READ, WRITE, DELETE, ALTER** (SAD:436-437, 505-507).
- ALLOW ACL, action DESCRIBE_CONFIGS: matches if ACL op is **DESCRIBE_CONFIGS or ALTER_CONFIGS** (SAD:442-443, 508-509).
- ALLOW ACL, any other action op: ACL op must be equal (SAD:511-514).
- DENY ACL (not ALL): action op must be **equal** (SAD:517). A DENY READ does **not** deny DESCRIBE. Implication is
  ALLOW-only (SAD:500-501 comment).
- CREATE, ALTER_CONFIGS->DESCRIBE, IDEMPOTENT_WRITE, CLUSTER_ACTION, TWO_PHASE_COMMIT, CREATE_TOKENS, DESCRIBE_TOKENS imply
  nothing. ALL implies everything.
- Concrete operation codes: UNKNOWN 0, ANY 1, ALL 2, READ 3, WRITE 4, CREATE 5, DELETE 6, ALTER 7, DESCRIBE 8,
  CLUSTER_ACTION 9, DESCRIBE_CONFIGS 10, ALTER_CONFIGS 11, IDEMPOTENT_WRITE 12, CREATE_TOKENS 13, DESCRIBE_TOKENS 14,
  TWO_PHASE_COMMIT 15 (`clients/.../acl/AclOperation.java:45-120`). Resource types: UNKNOWN 0, ANY 1, TOPIC 2, GROUP 3,
  CLUSTER 4, TRANSACTIONAL_ID 5, DELEGATION_TOKEN 6, USER 7 (`.../resource/ResourceType.java:31-66`). Pattern types:
  UNKNOWN 0, ANY 1, MATCH 2, LITERAL 3, PREFIXED 4 (`PatternType.java:33-67`). Permission: UNKNOWN 0, ANY 1, DENY 2, ALLOW 3
  (`AclPermissionType.java:31-46`).
- Supported operations per resource type (what a request can ever ask, and what authorized-operations bitfields list;
  AclEntry:53-63): TOPIC {READ, WRITE, CREATE, DESCRIBE, DELETE, ALTER, DESCRIBE_CONFIGS, ALTER_CONFIGS}; GROUP {READ,
  DESCRIBE, DELETE, DESCRIBE_CONFIGS, ALTER_CONFIGS}; CLUSTER {CREATE, CLUSTER_ACTION, DESCRIBE_CONFIGS, ALTER_CONFIGS,
  IDEMPOTENT_WRITE, ALTER, DESCRIBE}; TRANSACTIONAL_ID {DESCRIBE, WRITE, TWO_PHASE_COMMIT}; DELEGATION_TOKEN {DESCRIBE};
  USER {CREATE_TOKENS, DESCRIBE_TOKENS}. Nothing stops CreateAcls from storing an ACL whose operation is meaningless for
  its resource type (e.g. TOPIC/IDEMPOTENT_WRITE); it simply never matches.
- The cluster resource has one name, `kafka-cluster` (`Resource.CLUSTER_NAME`, `clients/.../resource/Resource.java:32`).
- Denial error type per resource type (`AclEntry.authorizationError`, AclEntry:65-74): TOPIC 29, GROUP 30, CLUSTER 31,
  TRANSACTIONAL_ID 53, DELEGATION_TOKEN 65. Default messages (Errors:244-246, 296): "Topic authorization failed.",
  "Group authorization failed.", "Cluster authorization failed.", "Transactional Id authorization failed.".

### 1.5 `authorizeByResourceType` (used by InitProducerId, 2.x row)

`Authorizer.authorizeByResourceType(ctx, op, type)` (Authorizer:184-293). Used only for `WRITE` on `TOPIC` in
InitProducerId (KA:1616-1617). Algorithm:
1. Authorize `op` on LITERAL `type:"hardcode"` as a normal action (so super users pass, and so does anybody when the
   `allow.everyone.if.no.acl.found` default applies to a resource named "hardcode"). If ALLOWED -> ALLOWED.
2. Else read all ACLs of that resource type (any pattern, any name) and keep those whose host is `*` or equal, whose
   principal is the session principal (parsed) or the string `User:*`, and whose op is **equal to `op` or ALL** (no
   implication). DENY LITERAL `*` -> immediate DENIED. Collect DENY LITERAL names, DENY PREFIXED names, ALLOW LITERAL names,
   ALLOW PREFIXED names; an ALLOW LITERAL `*` -> ALLOWED (checked after the deny scan).
3. An ALLOW (literal or prefixed) counts unless dominated: a literal allow `N` is dominated by a literal deny `N`; any
   allow string is dominated if any **PREFIXED deny** equals a prefix of it (including itself). First undominated allow
   -> ALLOWED. Otherwise DENIED.
4. There is no consideration of `allow.everyone.if.no.acl.found` beyond step 1's hard-coded name.

### 1.6 Authorized-operations bitfields

`AuthHelper.authorizedOperations` (AH:62-76): for each *supported operation of the resource type* (list in 1.4), one
`authorize` call with `logIfAllowed=false, logIfDenied=false`; result is a bitfield with bit `1 << opCode` per allowed
operation (`Utils.to32BitField`: `value |= 1 << code`, `clients/.../utils/Utils.java:1367-1372`). ALL is never a bit.
Without an authorizer every supported op's bit is set. Where requested:
- Metadata (KA:960-981): v8-v10 cluster ops when `IncludeClusterAuthorizedOperations` (if DESCRIBE on cluster is denied the
  value is `0`; if not requested `Int.MinValue`, the schema default); topic ops when `IncludeTopicAuthorizedOperations` for
  every returned topic entry (including error entries, computed by topic name).
- DescribeCluster (AH:149-156): same cluster rule, returns 0 when DESCRIBE denied, `Int.MinValue` when not asked.
- DescribeGroups v3+ when `IncludeAuthorizedOperations`, only for groups without error (KA:1326-1335);
  ConsumerGroupDescribe likewise (KA:2675-2684).

### 1.7 ACL APIs: CreateAcls / DeleteAcls / DescribeAcls

Versions in 4.3.1: all three **1-3** (`msg/CreateAclsRequest.json`, `DeleteAclsRequest.json`, `DescribeAclsRequest.json`
`validVersions`). v0 was removed; Kafka closes the connection on a v0 request (recorded: `versions-not-served`, claim V2).
Vendored crate agrees (`vendor/kafka-protocol/src/messages/*_acls_request.rs` `VERSIONS min 1 max 3`).

**Authorization of the call itself** (AclApis:62-64, 84-86, 140-142): DescribeAcls requires `DESCRIBE` on CLUSTER
`kafka-cluster`; CreateAcls and DeleteAcls require `ALTER` on CLUSTER. Failure is `ClusterAuthorizationException`
thrown out of the handler and turned by `handleError` into the API's `getErrorResponse` (see 2.0):
- DescribeAcls: top-level `ErrorCode=31`, `ErrorMessage` = `Request <RequestChannel.Request.toString> needs DESCRIBE permission.`
  (`AuthHelper.authorizeClusterOperation`, AH:57-60; `ApiError.fromThrowable` keeps a message that differs from the default,
  `clients/.../requests/ApiError.java:38-46`; `DescribeAclsRequest.getErrorResponse`, DescribeAclsRequest.java:95-103).
  `Request.toString` is `Request(processor=N, connectionId=..., session=..., listenerName=..., securityProtocol=...,
  buffer=..., envelope=...)` (`RequestChannel.scala:290-296`) - not reproducible; tests must not pin this text. Same
  mechanism for CreateAcls (every result gets code 31 + message "... needs ALTER permission.",
  `CreateAclsRequest.getErrorResponse`, CreateAclsRequest.java:76-83) and DeleteAcls (every filter result,
  DeleteAclsRequest.java:106-116).
- Hence the port should answer 31 and a stable message of the same shape; the exact bytes cannot match.

**Parse-time rules** (the request object is built when the frame is read; a throw there closes the connection - no
response; `RequestChannel.scala:86`, `SS:1027-1056` `processChannelException` closes the channel):
- DescribeAcls (DescribeAclsRequest.java:70-88), DeleteAcls (DeleteAclsRequest.java:85-95), CreateAcls
  (CreateAclsRequest.java:97-103) throw `IllegalArgumentException` when the **raw wire code is 0** (`UNKNOWN`) in any of:
  resource type, pattern type, operation, permission type. -> connection closed, no answer. **Only the literal code 0:** a code
  that is simply not a defined value (e.g. 99, -1) passes this check and is mapped to UNKNOWN later by `fromCode`
  (`PatternType.fromCode`: `getOrDefault(code, UNKNOWN)`, PatternType.java:111-113) - see the next bullets. Recorded:
  `acl-unknown-codes` (api.md V5): `describe-resource-0`, `delete-permission-0`, `create-operation-0` etc. -> connection closed;
  the `-99` and `-negative` steps are handled as described in the next bullets.
- Response side builds with the same rule (`DeleteAclsResponse.validate`, DeleteAclsResponse.java:90-107).

**DescribeAcls handling** (AclApis:73-80): `filter = DescribeAclsRequest.filter` (`AclBindingFilter`), response
`DescribeAclsResponse.aclsResources(authorizer.acls(filter))`. No validation of the filter: a filter with unknown (but
nonzero) codes simply matches nothing (recorded `describe-*-99` -> `ErrorCode 0, ErrorMessage "", Resources null`).
Success `ErrorMessage` is the empty string `""` and `Resources` null/empty when nothing matches (recorded).
Result grouping (DescribeAclsResponse.java:139-164): bindings grouped by `ResourcePattern` into `HashMap<pattern,
HashSet<entry>>` -> **unspecified order** of resources and of ACL entries within a resource. Clients must not rely on it; the
port may choose a deterministic order.

**Filter matching** (`AclBindingFilter.matches` = pattern filter AND entry filter; used by both Describe and Delete,
`AclCache.acls` AclCache.java:56-65, `AclControlManager.deleteAclsForFilter` ACM:198-217):
- `ResourcePatternFilter.matches(pattern)` (ResourcePatternFilter.java:89-116):
  (a) resource type: filter ANY or equal. (b) pattern type: filter ANY or MATCH, or equal to the ACL's. (c) name null ->
  match. (d) if filter pattern type is ANY, or equals the ACL's -> `filterName.equals(aclName)` (exact; a PREFIXED filter does
  *not* do prefix matching on PREFIXED ACLs). (e) otherwise (filter type MATCH): ACL LITERAL -> `filterName.equals(aclName) ||
  aclName == "*"`; ACL PREFIXED -> `filterName.startsWith(aclName)`.
  So `MATCH` = "which ACLs would apply to resource `filterName`": literal-exact, wildcard `*`, and covering prefixes.
- `AccessControlEntryFilter.matches(entry)` (AccessControlEntryFilter.java:100-108): principal null = any, else
  `equals`; host null = any, else `equals` (a filter host of `*` matches only ACLs whose host is the literal `*`); operation ANY or
  equal; permission ANY or equal. Exact, case-sensitive strings.
- `ResourcePattern` constructor (ResourcePattern.java:43-55) forbids ANY resource type and ANY/MATCH pattern type; `AccessControlEntry`
  (AccessControlEntry.java:36-46) forbids ANY operation and ANY permission and null principal/host; these constructors run when
  CreateAcls items are turned into bindings (below).

**CreateAcls handling** (AclApis:84-138, validation ACM:95-177):
1. Cluster `ALTER` check (above). Then `allBindings = creations.map(CreateAclsRequest.aclBinding)` (AclApis:94):
   building `ResourcePattern`/`AccessControlEntry` throws `IllegalArgumentException` for **resource type ANY (1), pattern type
   ANY (1) or MATCH (2), operation ANY (1), permission ANY (1)** (ResourcePattern.java:48-54; AccessControlEntry.java:40-44),
   *before any per-item processing* -> the whole request fails via `handleError` -> every result `ErrorCode -1`
   (`UNKNOWN_SERVER_ERROR`) with `ErrorMessage null` (generic exception, message suppressed: ApiError.java:43-44).
   Recorded: `create-resource-any`, `create-pattern-any`, `create-pattern-match`, `create-operation-any`,
   `create-permission-any` -> `ErrorCode -1, ErrorMessage null` (api.md V5). In a multi-item request one such item turns *all* items
   into -1.
2. Per item, in the handler (AclApis:97-110), in this order: resource type CLUSTER and name != `kafka-cluster` ->
   `INVALID_REQUEST` (42) `The only valid name for the CLUSTER resource is kafka-cluster`; else empty resource name ->
   `INVALID_REQUEST` `Invalid empty resource name`. These items get their error and are not sent to the controller.
3. Remaining items go to the controller (`AclControlManager.createAcls`, ACM:95-125) and per item `validateNewAcl`
   (ACM:135-177) in this order, each `INVALID_REQUEST` (42):
   `Invalid resourceType UNKNOWN` / `Invalid resourceType ANY`; `Invalid patternType <X>` (anything but LITERAL, PREFIXED);
   `Invalid operation UNKNOWN` / `ANY`; `Invalid permissionType <X>` (anything but DENY, ALLOW); `Resource name should not be
   empty` (unreachable after step 2); principal without `:` ->
   ``Could not parse principal from `<principal>` (no colon is present separating the principal type from the principal name)``.
   Recorded for raw code 99: `Invalid resourceType UNKNOWN`, `Invalid patternType UNKNOWN`, `Invalid operation UNKNOWN`,
   `Invalid permissionType UNKNOWN`.
   Nothing else is validated: host may be any string including empty; principal `User:` (empty name) and `User:*` pass; a
   resource name may contain anything; a PREFIXED ACL with name `*` is allowed (it is a prefix of the literal character `*`).
4. Idempotence and duplicates: a binding equal to an existing ACL, or equal to an earlier one in the same request, is **not
   stored twice and reports success** (ACM:109-120); the result list always has one entry per request item in request order
   (AclApis:116-127). Success entries: `ErrorCode 0`, `ErrorMessage ""` (empty string) (recorded: `create-good`).
5. Atomicity: the controller result is atomic over the whole request (`ControllerResult(records, results, true)`), per-item
   failures excluded. More than 10000 new distinct ACLs in one request overflows the bounded record list
   (`MAX_RECORDS_PER_USER_OP = 10000`, QuorumController.java:179, 186; ACM:98-99) -> unspecified failure (see Unknowns).

**DeleteAcls handling** (AclApis:140-171, ACM:179-226):
- Cluster `ALTER` check. For each filter, in order: `validateFilter` (ACM:219-226): `patternFilter.isUnknown()` ->
  `INVALID_REQUEST` `Unknown patternFilter.` (resource type or pattern type UNKNOWN); `entryFilter.isUnknown()` ->
  `INVALID_REQUEST` `Unknown entryFilter.` (operation or permission UNKNOWN). Reachable only with raw codes other than 0
  (recorded: `delete-resource-99`, `delete-pattern-99`, `delete-operation-99`, `delete-permission-99`, `delete-resource-negative`).
- Otherwise delete **every** ACL the filter matches; the filter result lists them as `MatchingAcls` with `ErrorCode 0`
  (`DeleteAclsResponse.filterResult`, DeleteAclsResponse.java:109-138). A filter that matches nothing is success with an empty
  list (recorded: `delete-any` -> `ErrorCode 0, ErrorMessage null, MatchingACLs null`; note success message is **null** here, `""`
  in CreateAcls/DescribeAcls).
- Each filter is evaluated against the ACLs as they were **before** this request (the controller scans `idToAcl`, which is not
  changed until the records are applied, ACM:198-217): two filters that match the same ACL both list it in `MatchingAcls`, and the ACL
  is removed once (the removal records are a `Set`, ACM:181, 195). Order of matched ACLs inside a filter result is hash-map order
  (unspecified).
- More than 10000 removals in one request fails the **whole request** with `INVALID_REQUEST` `Cannot remove more than 10000 acls in a
  single delete operation.` (ACM:187-190, 207-210).
- Filters are not required to be "definite"; `ANY` filters (everything) are fine (recorded `delete-any`).

**Effect on enforcement**: ACL changes are visible to the authorizer as soon as the metadata record is applied; for a one-process port
the next request after the CreateAcls/DeleteAcls response sees them (Kafka's response is sent after the records are committed,
`ClusterMetadataAuthorizer.createAcls` completes futures after persistence, ClusterMetadataAuthorizer.java:95-98, 135-138).

### 1.8 Logging side effect

Allowed/denied audit lines are logged under logger `kafka.authorizer.logger` (SAD:113-115, 279-308): denied at INFO if
`logIfDenied`, allowed at DEBUG if `logIfAllowed`. `Action.logIfDenied=false` for filter-style checks. Not wire-observable.

---

## 2. Authorization matrix

### 2.0 General shape rules

- **Handlers run only for authenticated channels.** Authentication (SASL) is finished in the network layer before a request reaches
  `KafkaApis` (3.2). A PLAINTEXT/SSL-without-SASL connection never authenticates and carries `User:ANONYMOUS` (or the DN).
- **Whole-request denial** is built with the request type's `getErrorResponse(Throwable)`: the same error at every item/partition of
  that API's shape; for APIs with a message field the message is `ApiError.fromThrowable`'s (null for default-message errors, i.e.
  all the `*_AUTHORIZATION_FAILED` created from `Errors.X.exception`; the `authorizeClusterOperation` ones carry the long message,
  1.7). `handleError` (RHH:46-52) is used for exceptions thrown out of a handler.
- **Per-item denial** appends or substitutes a per-topic/partition/group/id entry. Where Kafka merges an authorized response
  with an unauthorized one, the **order of entries in the response is not the order of the request** - noted per row.
- **Topic existence is hidden**: topic authorization is always checked *before* the metadata-cache existence check, with the same
  operation the API needs, so a principal without the right gets TOPIC_AUTHORIZATION_FAILED for a topic that does not exist exactly
  as for one that does. Exceptions are listed in 2.3.
- Authorization of an API happens **before** the API's semantic validation in nearly every handler, with the exceptions in 2.2.
- A request whose API/version Kafka does not serve is closed before any authorization (`SS:775-784`, 6.1).

### 2.1 The matrix

doze-kafka serves the APIs of `crates/kafka-wire/src/api.rs` `served_apis!` (last column = the 4.3 profile). "Resource+op" uses
`TOPIC:READ` = ACL resource type TOPIC, operation READ. "Denied -> shape" gives the error and where it lands.

| API (key; doze versions) | Resource + operation | Denied -> error shape | Order / notes | Source |
|---|---|---|---|---|
| Produce (0; 3-9) | (a) if any batch is transactional: `TRANSACTIONAL_ID:WRITE` on `transactionalId` (null id -> denied); (b) per topic: `TOPIC:WRITE` | (a) whole request: every partition `TRANSACTIONAL_ID_AUTHORIZATION_FAILED`(53) via `getErrorResponse`; (b) per partition `TOPIC_AUTHORIZATION_FAILED`(29) | (a) before (b); (b) before existence (`UNKNOWN_TOPIC_OR_PARTITION`) and before record validation. **No IDEMPOTENT_WRITE check on Produce** - an idempotent producer needs only topic WRITE here. `acks=0` + any error: no response, **connection closed** (`ProduceRequest.getErrorResponse` returns null for acks 0, `ProduceRequest.java:166`; KA:507-521). Response lists the authorized topics' results merged with unauthorized/nonexistent/invalid (`responseStatus ++ unauthorized ++ nonExisting ++ invalid`) | KA:396-450, 457-528 |
| Fetch (1; 4-13) | consumer (`replicaId < 0`): `TOPIC:READ` per topic; `replicaId >= 0` (follower): `CLUSTER:CLUSTER_ACTION` once | consumer: partition `TOPIC_AUTHORIZATION_FAILED`; follower w/o CLUSTER_ACTION: **every** partition `TOPIC_AUTHORIZATION_FAILED` | fetch-session bookkeeping (`fetchManager.newContext`) runs before authorization (KA:576); v13 unknown topic id -> `UNKNOWN_TOPIC_ID` before authz; authz before existence; erroneous partitions are appended after data partitions in the response map (KA:670) | KA:563-620; `FetchRequest.isFromFollower` = `replicaId>=0` (FetchRequest.java:439-441) |
| ListOffsets (2; 1-7) | `TOPIC:DESCRIBE` per topic | partition entries `TOPIC_AUTHORIZATION_FAILED`, `Timestamp -1, Offset -1` | response = authorized topics' results then unauthorized topics appended (KA:805-807); authz before existence | KA:781-821 |
| Metadata (3; 0-12) | `TOPIC:DESCRIBE` per requested topic (also per topic id, v10+); auto-create path: `CLUSTER:CREATE` else `TOPIC:CREATE`; v8-10 `CLUSTER:DESCRIBE` for cluster ops; topic ops by name | requested topic: topic entry with `TOPIC_AUTHORIZATION_FAILED`, name set (by-id request: name null, topic id as supplied/known), no partitions; auto-create denied: `TOPIC_AUTHORIZATION_FAILED`. **All-topics request (null/`[]` at v0, null at v1+): unauthorized topics silently omitted** (KA:937-938) | describe check precedes existence, so existence of unauthorized names is hidden. `logIfDenied` is false for all-topics. Auto-create attempted only if `AllowAutoTopicCreation && auto.create.topics.enable` and the topic is authorized-for-describe and missing; deny -> 29 instead of `UNKNOWN_TOPIC_OR_PARTITION`. Response entries ordered: unknown ids, found topics, create-denied, describe-denied (KA:983-984) | KA:880-1002 |
| OffsetCommit (8; 2-9) | `GROUP:READ`, then per topic `TOPIC:READ` | group: whole request `GROUP_AUTHORIZATION_FAILED`(30) via `getErrorResponse`; topic: every partition of the topic `TOPIC_AUTHORIZATION_FAILED` | group before topics; topic authz before `UNKNOWN_TOPIC_OR_PARTITION` / unknown topic id handling (v9 `UNKNOWN_TOPIC_ID` comes first only when the id cannot be resolved to a name, KA:302-306) | KA:273-374 |
| OffsetFetch (9; 1-9) | per group `GROUP:DESCRIBE`; per requested topic `TOPIC:DESCRIBE` | group denied: v8+ that group's entry `ErrorCode 30`; v2-7 top-level `ErrorCode 30`; v1 each requested partition `ErrorCode 30, Offset -1` (`OffsetFetchResponse.groupError`, :229-254, `TOP_LEVEL_ERROR_AND_NULL_TOPICS_MIN_VERSION = 2`); topic denied (explicit topics): that topic's partitions `TOPIC_AUTHORIZATION_FAILED`, `CommittedOffset -1`; **all-partitions request (topics null): unauthorized topics silently dropped** | group first; topic check on the *result* for all-partitions, on the *request* for explicit lists. Error topics appended after found topics (KA:1180-1182) | KA:1018-1185 |
| FindCoordinator (10; 0-4) | key type GROUP: `GROUP:DESCRIBE`; key type TRANSACTION: `TRANSACTIONAL_ID:DESCRIBE` | v4: that key's coordinator entry error 30 / 53, node `-1` (`Node.noNode`); v0-3: whole response `sendErrorResponseMaybeThrottle` with the error (host empty, port -1, node -1) | authz before coordinator lookup (`COORDINATOR_NOT_AVAILABLE`) | KA:1187-1300 |
| JoinGroup (11; 0-9) | `GROUP:READ` | whole request `GROUP_AUTHORIZATION_FAILED` | before coordinator logic (no topic check on subscription metadata) | KA:1376-1398 |
| Heartbeat (12; 0-4) | `GROUP:READ` | whole request error 30 | | KA:1466-1484 |
| LeaveGroup (13; 0-5) | `GROUP:READ` | whole request error 30 | | KA:1486-1504 |
| SyncGroup (14; 0-5) | `GROUP:READ` | whole request error 30 | **ordering exception**: protocol type/name presence check (`INCONSISTENT_GROUP_PROTOCOL`) runs *before* authorization (KA:1406-1411) | KA:1400-1426 |
| DescribeGroups (15; 0-5) | per group `GROUP:DESCRIBE` | that group's entry: `ErrorCode 30`, `GroupState ""`, empty protocol type/protocol/members, authorized-ops omitted (`DescribeGroupsResponse.groupError`, :107-110, `UNKNOWN_STATE = ""`) | unauthorized entries come **first** in the response, authorized results appended (KA:1337-1343); `IncludeAuthorizedOperations` (v3+) bitfield per authorized group | KA:1302-1348 |
| ListGroups (16; 0-5) | `CLUSTER:DESCRIBE` (no log if denied) else per-group `GROUP:DESCRIBE` filter | never an error: groups the principal cannot DESCRIBE are **omitted** (if cluster DESCRIBE allowed, all shown) | filter applied after the coordinator lists | KA:1350-1374 |
| DeleteGroups (42; 0-2) | per group `GROUP:DELETE` (request group names de-duplicated first) | that group `ErrorCode 30` | response = authorized results then unauthorized appended (KA:1456-1460) | KA:1428-1464 |
| OffsetDelete (47; 0) | `GROUP:DELETE`, then per topic `TOPIC:READ` | group: whole request error 30; topic: all partitions `TOPIC_AUTHORIZATION_FAILED` | group, topics, then existence | KA:2363-2427 |
| ApiVersions (18; 0-4) | none | - | not authorized; pre-auth allowed once on a SASL listener (3.2). Kafka advertises SaslHandshake and SaslAuthenticate unconditionally (`ApiVersionsResponse.filterApis`, ApiVersionsResponse.java:195-205) - doze's `when Sasl` hiding is a deliberate deviation to keep | KA:1518-1536 |
| SaslHandshake (17; doze 1 only, when SASL on) / SaslAuthenticate (36; 0-2) | none | reaching `KafkaApis` means the channel is already authenticated (or the listener has no SASL): `ILLEGAL_SASL_STATE` (34); SaslHandshake with an empty mechanisms array, SaslAuthenticate with message `SaslAuthenticate request received after successful authentication` (3.7) | during the login itself these are handled in the network layer (3.2), never here | KA:1506-1516 |
| CreateTopics (19; 2-7) | `CLUSTER:CREATE` once, else per topic `TOPIC:CREATE`; configs/numPartitions in the result need `TOPIC:DESCRIBE_CONFIGS` | topic `TOPIC_AUTHORIZATION_FAILED` with message `Authorization failed.`; success without `DESCRIBE_CONFIGS`: `TopicConfigErrorCode = TOPIC_AUTHORIZATION_FAILED(29)`, no configs, `NumPartitions`/`ReplicationFactor` omitted (-1) | **unauthorized topics are removed before the controller sees the request** (so `TOPIC_ALREADY_EXISTS` is never revealed to someone without CREATE). Response order: controller results for authorized topics (request order), then duplicate-name errors (`INVALID_REQUEST` `Duplicate topic name.`), then `__cluster_metadata` (`INVALID_REQUEST` `Creation of internal topic __cluster_metadata is prohibited.`) and unauthorized topics (set order). Duplicated names never reach authz | CA:362-449; ReplicationControlManager.java:824-843 |
| DeleteTopics (20; 1-6) | `CLUSTER:DELETE` once (grants both) else per topic: `TOPIC:DESCRIBE` and `TOPIC:DELETE` | topic not describable: `TOPIC_AUTHORIZATION_FAILED` even if it does not exist (name echoed for by-name requests; for by-id requests name null); describable but not deletable: `TOPIC_AUTHORIZATION_FAILED` (with the real name/id); describable and absent: `UNKNOWN_TOPIC_OR_PARTITION` | `delete.topic.enable=false` is checked **first**, before any authz (`TopicDeletionDisabledException` v3+, `InvalidRequestException` "This version does not support topic deletion." <v3, CA:231-237); then name/id duplicate/shape errors (INVALID_REQUEST `Duplicate topic name.` etc.) - not hidden by authz; response order is **randomly shuffled** (`Collections.shuffle`, CA:355) | CA:198-360 |
| DeleteRecords (21; 0-2) | per topic `TOPIC:DELETE` | partition `TOPIC_AUTHORIZATION_FAILED`, `LowWatermark -1` | authz before existence; response grouped by topic from an unordered map | KA:1538-1601 |
| InitProducerId (22; 0-4) | transactional id present: `TRANSACTIONAL_ID:WRITE`, plus `TRANSACTIONAL_ID:TWO_PHASE_COMMIT` if `Enable2Pc`; no id: `CLUSTER:IDEMPOTENT_WRITE` (log allowed, not denied) **or** `authorizeByResourceType(WRITE, TOPIC)` (1.5) | transactional: top-level `TRANSACTIONAL_ID_AUTHORIZATION_FAILED`(53); idempotent: top-level `CLUSTER_AUTHORIZATION_FAILED`(31) | authz before the producer-id/epoch consistency check (`INVALID_REQUEST`, KA:1644-1648). **A principal with WRITE on any topic (and nothing on the cluster) may init an idempotent producer** - recorded (`idempotent-write-from-topic-write`: alice WRITE on one topic -> 0 + producer id; bob none -> 31) | KA:1603-1666 |
| AddPartitionsToTxn (24; 0-3) | per transaction (v0-3: one): `TRANSACTIONAL_ID:WRITE`; then per partition topic `TOPIC:WRITE` (internal topics always fail) | txn denied: every partition of that transaction `TRANSACTIONAL_ID_AUTHORIZATION_FAILED`; topic denied: that partition `TOPIC_AUTHORIZATION_FAILED`; unknown: `UNKNOWN_TOPIC_OR_PARTITION`; **if any partition failed, every other partition is `OPERATION_NOT_ATTEMPTED`** | the topic filter excludes internal topics (`filterNot(Topic.isInternal)`) so an internal topic is `TOPIC_AUTHORIZATION_FAILED` even with **no authorizer**; v4+ (broker-only) requires `CLUSTER:CLUSTER_ACTION` instead | KA:1856-1966 |
| AddOffsetsToTxn (25; 0-3) | `TRANSACTIONAL_ID:WRITE`, then `GROUP:READ` | top-level `ErrorCode` 53, then 30 | txn id before group | KA:1968-2016 |
| EndTxn (26; 0-3) | `TRANSACTIONAL_ID:WRITE` | top-level 53 | before coordinator state checks | KA:1668-1709 |
| TxnOffsetCommit (28; 0-3) | `TRANSACTIONAL_ID:WRITE`, then `GROUP:READ`, then per topic `TOPIC:READ` | txn: all partitions 53; group: all partitions 30; topic: its partitions 29 | in that order; topic authz before `UNKNOWN_TOPIC_OR_PARTITION` | KA:2018-2116 |
| DescribeAcls (29; 1-3) | `CLUSTER:DESCRIBE` | top-level `ErrorCode 31` + long message (1.7) | see 1.7 | AclApis:62-82 |
| CreateAcls (30; 1-3) | `CLUSTER:ALTER` | every result `ErrorCode 31` + long message | see 1.7; validation after auth | AclApis:84-138 |
| DeleteAcls (31; 1-3) | `CLUSTER:ALTER` | every filter result `ErrorCode 31` + long message | | AclApis:140-171 |
| DescribeConfigs (32; 1-4) | per resource: BROKER / BROKER_LOGGER / CLIENT_METRICS -> `CLUSTER:DESCRIBE_CONFIGS`; TOPIC -> `TOPIC:DESCRIBE_CONFIGS` on the name; GROUP -> `GROUP:DESCRIBE_CONFIGS` | that resource's result: `ErrorCode` 31 / 29 / 30, `ErrorMessage` = default message (`Cluster authorization failed.` etc.), no configs, name/type echoed | authorized resources' results come **first**, unauthorized appended (CH:79); an unknown resource type throws `InvalidRequestException` (whole request `INVALID_REQUEST`, CH:62, 71); authz before existence of the topic (existence is judged inside `describeConfigs` for authorized ones only) | CH:49-80; KA:2209-2213 |
| AlterConfigs (33; 0-2) | per resource: BROKER / CLIENT_METRICS -> `CLUSTER:ALTER_CONFIGS`; TOPIC -> `TOPIC:ALTER_CONFIGS`; GROUP -> `GROUP:ALTER_CONFIGS`; other types `INVALID_REQUEST` `Unexpected resource type X.` | that resource: `ErrorCode` 31 / 29 / 30 with the default message (`apiError.message()` is the `ApiError(CODE)` message = default) | **broker-side preprocessing runs before authorization**: duplicate resource -> `INVALID_REQUEST` `Each resource must appear at most once.` (both copies), duplicate config key `Error due to duplicate config keys`, null value `Null value not supported for : a, b`, BROKER name/validation errors (`Node id must be an integer...`, `Unexpected broker id, expected N, but received M`), `Unknown resource type N` - an unauthorized principal sees these before any 31 (CAM:209-261). In `ControllerApis` (CA:497-554): UNKNOWN type -> `UNSUPPORTED_VERSION`(35) `Unknown resource type N.`; duplicate -> `INVALID_REQUEST` `Duplicate resource.`; then authorization per resource, then controller validation | CAM:209-261; CA:472-554; KA:2158-2180 |
| CreatePartitions (37; 0-3) | per topic `TOPIC:ALTER` | topic result `TOPIC_AUTHORIZATION_FAILED`(29) with **no** message | duplicate names -> `INVALID_REQUEST` `Duplicate topic name.` (both removed) before authz; authz before existence/validation, so absent topics are 29 without ALTER; results = duplicate errors, unauthorized, then controller results (CA:840-861) | CA:796-861 |
| IncrementalAlterConfigs (44; 0-1) | like AlterConfigs; additionally BROKER_LOGGER -> `CLUSTER:ALTER_CONFIGS` (broker preprocessing, CAM:133-137) / `CLUSTER:CLUSTER_ACTION` in the controller path (CA:730) | like AlterConfigs; BROKER_LOGGER denial -> `CLUSTER_AUTHORIZATION_FAILED` from `RuntimeLoggerManager` | preprocessing (duplicates `Each resource must appear at most once.`, `Error due to duplicate config keys`, `Null value not supported for : ...`, BROKER validation, `Unknown resource type N` as `INVALID_REQUEST`) **before** authorization (CAM:100-160); then CA:715-794 per-resource authorization | CAM:100-160; CA:715-794; KA:2182-2207 |
| DescribeCluster (60; 0-2) | **none** (only `CLUSTER:DESCRIBE` to fill `ClusterAuthorizedOperations` when requested) | not denied; authorized-ops = 0 when DESCRIBE denied | endpoint-type mismatch -> `INVALID_REQUEST` (v0) / `MISMATCHED_ENDPOINT_TYPE` with message `The request was sent to an endpoint of type BROKER, but we wanted an endpoint of type X`; `UNSUPPORTED_ENDPOINT_TYPE` `Unsupported endpoint type N` | AH:122-175; KA:2457-2483 |
| DescribeProducers (61; 0) | per topic `TOPIC:READ` | each requested partition: `TOPIC_AUTHORIZATION_FAILED` with `ErrorMessage = ApiError message` | order per topic: topic-name validity (`INVALID_TOPIC_EXCEPTION`) -> READ -> existence | KA:2485-2529 |
| DescribeTransactions (65; 0) | per transactional id `TRANSACTIONAL_ID:DESCRIBE`; then per topic in the state `TOPIC:DESCRIBE` | id denied: that entry `ErrorCode 53`; topics the principal cannot DESCRIBE are **silently removed** from an otherwise-visible state | | KA:2540-2566 |
| ListTransactions (66; 0-1) | per listed state `TRANSACTIONAL_ID:DESCRIBE` | never an error: unauthorized ids **omitted** | filter applied after coordinator listing | KA:2568-2593 |
| ConsumerGroupHeartbeat (68; 0-1) | `GROUP:READ`; if `SubscribedTopicNames` non-empty: `TOPIC:DESCRIBE` on each | group: whole request error 30; topics: `TOPIC_AUTHORIZATION_FAILED` top-level if **any** subscribed topic is not describable | feature check (`UNSUPPORTED_VERSION` if the protocol is disabled) before authz | KA:2604-2642 |
| ConsumerGroupDescribe (69; 0-1) | per group `GROUP:DESCRIBE`; afterwards every topic named in member assignments / target assignments `TOPIC:DESCRIBE` | group: entry `ErrorCode 30`; if the group's members hold a topic the principal cannot describe: entry replaced by `ErrorCode 29`, `ErrorMessage "The group has described topic(s) that the client is not authorized to describe."`, no members | unauthorized groups first in response | KA:2644-2728 |
| GetTelemetrySubscriptions (71) / PushTelemetry (72) | **none** | - | recorded: a user with no ACLs gets 0 (traffic.md R2) | KA:2969-2987 |
| DescribeUserScramCredentials (50) - **not in doze's served list; exists in the vendored crate** | `CLUSTER:DESCRIBE` | top-level `ErrorCode 31` via `getErrorResponse(CLUSTER_AUTHORIZATION_FAILED.exception)` | authz before lookup | KA:2444-2455 |
| AlterUserScramCredentials (51) - same | `CLUSTER:ALTER` (controller) | `authorizeClusterOperation` throws -> error response via `handleError` (code 31, long message) | authz before the per-user rules of 3.6 | CA:890-900 |

Notes on the table:
- "Resource names for authorization are the topic name, group id, transactional id as sent." The empty string or odd names are
  authorized like any other literal.
- Share/streams groups, delegation tokens, log dirs, quotas, features, raft, reassignment, leader election are served by Kafka
  but not by doze-kafka; their rows are in KA/CA if ever needed (e.g. `DescribeClientQuotas` needs `CLUSTER:DESCRIBE_CONFIGS`,
  `AlterClientQuotas` `CLUSTER:ALTER_CONFIGS`, `ElectLeaders`/`AlterPartitionReassignments`/`UpdateFeatures`
  `CLUSTER:ALTER`).

### 2.2 Order of authorization relative to other validation (observable)

1. Pre-handler (connection level): unsupported API/version or unparsable body closes the connection (no authz, no answer).
   An authorization-level answer only exists for requests that parse (6.1).
2. APIs where **validation precedes authorization**: SyncGroup (protocol type/name, KA:1406-1411); AlterConfigs and
   IncrementalAlterConfigs preprocessing (duplicate resources/keys, null values, BROKER config validity, unknown resource types:
   CAM:100-160, 209-261); DeleteTopics (`delete.topic.enable`, duplicate/shape checks, CA:231-283); CreateTopics duplicate-name
   removal; CreatePartitions duplicate-name removal (CA:840-846); DescribeConfigs unknown resource type; DescribeProducers topic-name
   validity (KA:2503-2505); Metadata `Topic name can not be null`/`Topic IDs are not supported` `InvalidRequestException` before
   authz (KA:885-893); Fetch session and unknown-topic-id handling (KA:567-610); InitProducerId nothing before authz.
3. Everywhere else authorization is first, then (for topic resources) existence, then deeper validation, then state errors.
4. Group APIs: group authorization first, then topic authorization (OffsetCommit, OffsetDelete, TxnOffsetCommit); for transactions
   transactional id first, then group, then topics.
5. Metadata for a topic that is both missing and denied: denied wins (29). Same for Produce, Fetch, ListOffsets, DeleteRecords,
   OffsetCommit, CreatePartitions, DeleteTopics (describe denied), DescribeConfigs (TOPIC), AddPartitionsToTxn, TxnOffsetCommit,
   OffsetDelete, DescribeProducers.

### 2.3 Rules where existence is NOT hidden, or is hidden differently

- OffsetFetch for all partitions (`Topics = null`): unauthorized topics vanish (no error entry) - existence of committed offsets for
  hidden topics is not revealed; explicit topics get 29.
- DescribeTransactions leaves a visible transaction's topic list filtered by DESCRIBE; ListTransactions/ListGroups filter silently.
- DeleteTopics: a principal with DESCRIBE but no DELETE on a topic gets 29 *with the topic's name and id*; with neither: 29 with
  the name only for by-name requests and **null name + id** for by-id requests (CA:314-320, 327-345).
- CreateTopics reveals the topic's config only with `DESCRIBE_CONFIGS` (otherwise `TopicConfigErrorCode=29`).
- Metadata by topic id for an unauthorized id: entry `TOPIC_AUTHORIZATION_FAILED`, name null, id = the id the cache knows (or zero if
  unknown to the cache) (KA:939-943). An unknown id answers `UNKNOWN_TOPIC_ID` (KA:903-904).
- Broker-side config preprocessing leaks validation text before authz (2.1, AlterConfigs rows).

---

## 3. SASL

### 3.1 Where it lives and when it runs

SASL authentication is in the **network layer** (`KafkaChannel.prepare()` -> `SaslServerAuthenticator.authenticate()`, KC:174-198),
driven by `Selector` for every read on a channel that is not yet `ready()` (`channel.ready() = transportLayer.ready() &&
authenticator.complete()`, KC:354-356). The processor never sees a request from an unauthenticated channel
(`SS:1002-1056` only receives completed requests from channels the selector reports as ready). Listeners whose security
protocol is `SASL_PLAINTEXT` or `SASL_SSL` use `SaslServerAuthenticator`; `PLAINTEXT` and `SSL` never do.

Mechanisms: `sasl.enabled.mechanisms` (default `GSSAPI` only - `BrokerSecurityConfigs.java:98-102`, so PLAIN/SCRAM must be listed
explicitly). In scope for doze: PLAIN, SCRAM-SHA-256, SCRAM-SHA-512.

### 3.2 Pre-authentication state machine (`SaslServerAuthenticator`, SSA:108-117, 250-304, 507-586)

States: `INITIAL_REQUEST` -> `HANDSHAKE_OR_VERSIONS_REQUEST` -> (`HANDSHAKE_REQUEST` after an ApiVersions) -> `AUTHENTICATE` ->
`COMPLETE` | `FAILED`.

On every complete frame in a state before `AUTHENTICATE`, `handleKafkaRequest` (SSA:507-547):
1. `RequestHeader.parse`. An unknown API key / unparsable header -> `InvalidRequestException`.
2. **Only `ApiVersions` and `SaslHandshake` are allowed**; anything else -> `InvalidRequestException("Unexpected Kafka request of type
   X during SASL handshake.")` (SSA:515-516). `InvalidRequestException` is not an `AuthenticationException`, so the connection
   is **closed at once with no response** (`SSA:298-302`, `Selector` closes non-auth exceptions, Sel:624-631). On the *first* frame
   of the connection (state `INITIAL_REQUEST`) the message is replaced by "Invalid request, potential reasons: kafka client
   configured with the wrong security protocol, it does not support KIP-43 or it is not a kafka client." (SSA:538-544; closed too).
   Recorded: `no-handshake-authenticate` (SaslAuthenticate before SaslHandshake) -> connection closed (A8).
3. The body is parsed (`requestContext.parseRequest`, throws `InvalidRequestException` on garbage), the principal in the context is
   `ANONYMOUS`.
4. State `INITIAL_REQUEST` -> `HANDSHAKE_OR_VERSIONS_REQUEST` once a *valid* Kafka request was parsed.
5. **ApiVersions** (SSA:572-586): only legal in `HANDSHAKE_OR_VERSIONS_REQUEST` - i.e. as the very first request. A second ApiVersions
   (state `HANDSHAKE_REQUEST`) throws `IllegalStateException` -> connection closed, no answer (recorded `versions-twice-second`,
   A8). Behaviour of the first one: unsupported request version -> response `UNSUPPORTED_VERSION` (35) at **v0** carrying only the
   ApiVersions key (ApiVersionsRequest.getErrorResponse; `RequestContext.apiVersion()` returns 0 for it, RequestContext.java:154-163),
   state unchanged (the client may retry); invalid software name/version (v3+) -> `INVALID_REQUEST` (42), state unchanged; else the
   full ApiVersions response and state `HANDSHAKE_REQUEST`. Throttle in these responses is 0.
6. **SaslHandshake** (SSA:549-565): the mechanism must be in `enabledMechanisms` (exact, case-sensitive: `plain` fails, `""` fails;
   recorded A6). Success: response `ErrorCode 0, Mechanisms = enabledMechanisms`; the `SaslServer` is created and state -> `AUTHENTICATE`.
   Failure: `UNSUPPORTED_SASL_MECHANISM` (33) with the same mechanism list, then the connection is closed (an `AuthenticationException`
   subclass, `UnsupportedSaslMechanismException`, so the response is delivered after `connection.failed.authentication.delay.ms`,
   3.3). Recorded A6: the next request on the connection is not answered.
   Handshake **version >= 1** switches the rest of the exchange to Kafka-framed `SaslAuthenticate` requests
   (`enableKafkaSaslAuthenticateHeaders(true)`, SSA:552-553). **Version 0 does not**: after the handshake response the client sends
   raw SASL tokens as `[int32 length][bytes]`, and the server replies the same way, with no Kafka header (SSA:421-432, 261-273).
   The first SaslHandshake can be sent as the first request (ApiVersions is optional).
7. A second SaslHandshake (or any non-SaslAuthenticate Kafka request) after the handshake but before the login finishes:
   state `AUTHENTICATE`, `enableKafkaSaslAuthenticateHeaders` true -> the frame is parsed; `apiKey != SASL_AUTHENTICATE` ->
   `IllegalSaslStateException("Unexpected Kafka request of type X during SASL authentication.")`; an **error response in that
   API's own shape** (`getErrorResponse(e)`: `ILLEGAL_SASL_STATE`(34) at every item) is queued, then sent after the failure delay,
   then the connection is closed (SSA:440-444). Recorded A9/A10 (Metadata: each topic 34 and no brokers; ApiVersions: 34 and
   no APIs; SaslHandshake: 34 with an empty mechanism array).
8. A `SaslAuthenticate` request with an **unsupported version** (>2): `UnsupportedVersionException`, closes immediately, no
   response (SSA:446-450).

SaslAuthenticate (v0-2) in state `AUTHENTICATE` (SSA:433-499): `authBytes` go to `SaslServer.evaluateResponse`. Success response
`ErrorCode 0, ErrorMessage` (recorded: empty string), `AuthBytes` = the mechanism's reply (empty array if none), `SessionLifetimeMs` =
0 while the exchange continues, and at completion `calcCompletionTimesAndReturnSessionLifetimeMs()` (3.7). Failures:
- `SaslAuthenticationException` from the mechanism: response `ErrorCode 58` (`SASL_AUTHENTICATION_FAILED`, default text "SASL
  Authentication failed.", Errors:308), `ErrorMessage = the exception's message` (SSA:474-480).
- Any other `SaslException`: response 58 with the **generic** message `Authentication failed during authentication due to invalid
  credentials with SASL mechanism <MECH>` (`during re-authentication` when re-authenticating) - the exception text is deliberately
  hidden (SSA:481-497).
- Any other exception type (e.g. `IllegalArgumentException` from SCRAM proof of the wrong length, 3.5): no response, connection closed
  (SSA:298-302; recorded A14).
After an error response is sent the connection is closed (3.3). The `SaslAuthenticate` response carries `AuthBytes` empty on errors.

### 3.3 Failure delivery and closing

- An `AuthenticationException` during authentication (`SaslAuthenticationException`, `UnsupportedSaslMechanismException`,
  `IllegalSaslStateException`, or a `SaslException` wrapped in `SaslAuthenticationException`) is caught in `KafkaChannel.prepare`
  (KC:183-193) and rethrown as `DelayedResponseAuthenticationException`; `Selector` then **delays the response and the close by
  `connection.failed.authentication.delay.ms`** (default `100`, `SocketServerConfigs.java:139`; Sel:628-629, 901-917) and
  after the delay sends the stored failure response and closes the channel gracefully. In `SASL v0` mode nothing is stored, so the
  connection is just closed after the delay with no answer.
- Non-authentication exceptions (`InvalidRequestException`, `IllegalStateException`, `IOException`, `InvalidReceiveException`
  not caught, malformed framing) close the channel immediately with no response (Sel:609-631).
- Pre-auth frame sizing: the authenticator reads with its own `NetworkReceive(saslAuthRequestMaxReceiveSize)`, default
  `sasl.server.max.receive.size = 524288` (SSA:195-197, 261; BrokerSecurityConfigs.java:118-122). A frame length < 0 or > 524288:
  `InvalidReceiveException` -> `SaslAuthenticationException("Failing SASL authentication due to invalid receive size")` (SSA:263-267;
  NetworkReceive.java:92-95 texts `Invalid receive (size = N)` / `... larger than 524288`). It is an `AuthenticationException`, but there
  is nothing to send (no stored response), so the observable effect is: close after the delay, no answer. Sub-limit tokens
  (64 KiB) are processed normally: recorded `token-of-64k-authenticate` -> 58 (A2).
  This cap applies only while a SASL listener is authenticating; after login the normal `socket.request.max.bytes`
  (default 100 MiB, `SocketServerConfigs.java:96`) applies via the Selector's `maxReceiveSize` (SS:800, 870).
- A channel's idle timeout (`connections.max.idle.ms`, 10 min, `SocketServerConfigs.java:135`) applies in the pre-auth phase too
  (same Selector).

### 3.4 SASL/PLAIN (`PlainSaslServer`)

Token: UTF-8 string `authzid NUL username NUL password` (Plain:85-89). Evaluation order, each failing as
`SaslAuthenticationException` (so message goes to the client verbatim, code 58):
1. Split on NUL into at most 4 pieces (Plain:116-134); `!= 3` pieces -> `Invalid SASL/PLAIN response: expected 3 tokens, got N`
   where N counts pieces: 1 for no NUL or empty token, 2, and 4 for a 4th piece or a trailing NUL (recorded A4).
2. `username.isEmpty()` -> `Authentication failed: username not specified`; then `password.isEmpty()` -> `Authentication failed:
   password not specified` (user checked first; both empty -> user message; recorded A3).
3. Credential verification through the callback handler: any exception -> `Authentication failed: credentials for user could not be
   verified`; not authenticated -> `Authentication failed: Invalid username or password` (same words for wrong password, unknown user, other
   case; recorded A2). Password compared in constant time (`Utils.isEqualConstantTime`).
4. Only after the password verifies: `authzid` non-empty and != username -> `Authentication failed: Client requested an authorization id
   that is different from username` (recorded A5: password is checked first).
5. Success: `evaluateResponse` returns an **empty byte array** (non-null): at SaslHandshake v0 the server sends a 4-byte zero length; at v1+ a
   SaslAuthenticate response with empty `AuthBytes`. Principal = `User:<username>`.
Invalid UTF-8 bytes are not an error of their own (they decode with replacement characters and fail as invalid credentials; recorded
`not-utf8-authenticate` -> 58, A2).

**Where PLAIN passwords come from in KRaft**: from the broker's JAAS configuration, not the SCRAM/metadata store.
`PlainServerCallbackHandler.authenticate` looks up the JAAS option `user_<username>` in the `PlainLoginModule` entry for the listener
(`JaasContext.configEntryOption(entries, "user_" + username, PlainLoginModule)`, PlainServerCallbackHandler.java:61-70). Selected
by `SaslChannelBuilder.java:327` (`new PlainServerCallbackHandler()`); can be replaced by `sasl.server.callback.handler.class`. The
password is compared exactly (no hashing); a missing `user_` entry -> not authenticated. So PLAIN users are static
configuration (doze: `--sasl-user user:password`). The KRaft SCRAM credential store is consulted only for SCRAM mechanisms
(`SaslChannelBuilder.java:329`).

### 3.5 SASL/SCRAM-SHA-256 and SCRAM-SHA-512

`ScramMechanism` (ScramMechanism.java:32-33): `SCRAM-SHA-256` (hash `SHA-256`, HMAC `HmacSHA256`, wire type 1) and `SCRAM-SHA-512`
(`SHA-512`, `HmacSHA512`, wire type 2); both `minIterations = 4096`, `maxIterations = 16384`.

Exchange (RFC 5802 profile, `ScramSaslServer`, ScramSaslServer.java:96-172), over two SaslAuthenticate round trips:
1. client-first: must match (ScramMessages.java:68-74)
   `n,(a=<authzid>)?,(m=<reserved>,)?n=<saslname>,r=<nonce><extensions>` where the GS2 header is **only `n`** (channel binding flags
   `y` and `p=` and unknown flags do not match), `saslname` is `(?:[\x01-\x7F&&[^=,]]|=2C|=3D)+`, the nonce is printable ASCII
   without comma (`[\x21-\x7E&&[^,]]+`), extensions are `,alpha=value` repeated. No match -> `SaslException("Invalid SCRAM client first
   message format: <message>")` -> generic 58 message (3.2) and close (recorded A15 for ~15 malformed shapes, including `y`, `p=`, a
   reserved `m=` accepted at this step). Extensions: only `tokenauth` is known; others are ignored with a debug log (delegation tokens
   out of scope).
2. username = `saslname` with `=2C` -> `,` and `=3D` -> `=`; a `=` not followed by 2C/3D -> `IllegalArgumentException("Invalid username:
   ...")` from `ScramFormatter.username` (ScramFormatter.java:110-116). It is thrown inside the `try` of ScramSaslServer.java:107-145, whose
   `catch (Throwable)` turns it into `SaslException("Authentication failed: Credentials could not be obtained")` (:143-145) -> generic
   58 message (recorded `first-user-bad-escape-client-first` -> 58, claim A15b).
3. Credential lookup by user name in the per-mechanism credential cache (`ScramServerCallbackHandler.handle`, ScramServerCallbackHandler.java:65-67). None ->
   `SaslException("Authentication failed: Invalid user credentials")` -> generic 58 message **at the client-first step**
   (recorded A12: an unknown user is detected at client-first, a wrong password only at client-final; the answer therefore reveals
   which users exist by timing of the failure).
4. `a=` authzid non-empty and != username -> `SaslAuthenticationException("Authentication failed: Client requested an authorization id
   that is different from username")` (message passes to the client verbatim) - evaluated *after* the credential was found (so an unknown
   user with an authzid gets the generic message; recorded A13).
5. Stored `iterations < 4096` -> `SaslException("Iterations N is less than the minimum 4096 for SCRAM-SHA-...")` -> generic message.
6. server-first = `r=<clientNonce><serverNonce>,s=<base64 salt>,i=<iterations>` (ScramMessages.java:177-179) with
   **serverNonce = `new BigInteger(130, SecureRandom).toString(36)`** (ScramFormatter.java:153-159): a base-36 string of up to 26
   characters. Salt and iteration count are the stored credential's (the server never generates a salt; the *client* of
   AlterUserScramCredentials does - the Java admin/`kafka-configs` uses `secureRandomBytes` = the UTF-8 bytes of another base-36 string
   of a 130-bit random number, ScramFormatter.java:161-167, so Kafka-made salts are ~26 ASCII bytes; any bytes are accepted).
7. client-final: `c=<base64>,r=<nonce>(,ext)*,p=<base64 proof>`. Parse failure -> `SaslException("Invalid SCRAM client final message
   format: ...")` -> generic 58. The channel-binding value (`c=`) is **base64-decoded but never compared** to the GS2 header
   (ScramMessages.java:205): `c=eSws` after `n,,` logs in (recorded A17). `r=` must equal the server-first nonce exactly else
   `SaslException("Invalid client nonce in the final client message.")` (generic 58).
8. Proof check (ScramSaslServer.java:227-237): `ClientSignature = HMAC(StoredKey, AuthMessage)`, `StoredKey' = H(ClientSignature XOR
   ClientProof)`, compared with `MessageDigest.isEqual` to the stored key; mismatch -> `SaslException("Invalid client credentials")` ->
   generic 58. `AuthMessage = client-first-bare + "," + server-first + "," + client-final-without-proof`
   (ScramFormatter.java:118-120, 134-138), where client-first-bare is rebuilt as `n=<saslname>,r=<nonce>[,<extension pairs>]`.
   **A proof whose byte length differs from the hash size makes `xor` throw `IllegalArgumentException`** ("Argument arrays must be of
   the same length", ScramFormatter.java:66-73) - not a `SaslException` -> no answer, connection closed (recorded A14). A proof that is not
   valid base64 fails the regex first -> generic 58 (recorded A16).
9. server-final = `v=<base64 ServerSignature>` with `ServerSignature = HMAC(ServerKey, AuthMessage)` (ScramMessages.java:281-286). The server never sends
   `e=` error attributes: failures are not SCRAM messages but the SaslAuthenticate error. After sending it the SaslServer state is COMPLETE;
   `SaslAuthenticate` response `ErrorCode 0`, `AuthBytes` = server-final.
10. Misuse inside the exchange: a client-final message sent first lands in state `RECEIVE_CLIENT_FIRST_MESSAGE`, fails the client-first
    regex (`SaslException`) -> generic 58; a second client-first message lands in `RECEIVE_CLIENT_FINAL_MESSAGE`, fails the client-final
    regex -> generic 58 (recorded A16). The `default` branch (`IllegalSaslStateException("Unexpected challenge in Sasl server state ...")`,
    ScramSaslServer.java:164-165) is only reachable when `evaluateResponse` is called in state COMPLETE/FAILED, which the authenticator
    prevents (it switches to `COMPLETE` and stops calling the server, SSA:255-258). Any SaslAuthenticate sent after the final message is
    therefore answered by `KafkaApis` with `ILLEGAL_SASL_STATE` (34) (3.7; recorded A18 `replay-client-final-again`).

Mechanism-name set for `SaslHandshake`: any other SCRAM name is `UNSUPPORTED_SASL_MECHANISM`. The vendored mapping `mechanism type
1 = SCRAM-SHA-256, 2 = SCRAM-SHA-512` is used in AlterUserScramCredentials/DescribeUserScramCredentials (3.6).

### 3.6 SCRAM credential administration (APIs 50 and 51)

Both exist in 4.3.1 (`msg/AlterUserScramCredentials*.json`, `DescribeUserScramCredentials*.json`: `validVersions 0`, flexible) and in the
vendored crate (`vendor/kafka-protocol/src/messages/{alter,describe}_user_scram_credentials_*.rs`, `VERSIONS min 0 max 0`).
**They are not in doze's `served_apis!`**, yet the recorded SCRAM scenario provisions users through AlterUserScramCredentials
(`conformance/claims/auth.md` header, `conformance/targets.go`), so doze must support at least that path for tests.

AlterUserScramCredentials (handler CA:890-900, logic SCM:175-322):
- Auth: `CLUSTER:ALTER` (`authorizeClusterOperation`), else the error response for the API.
- Request: `Deletions[{Name, Mechanism(int8)}]`, `Upsertions[{Name, Mechanism, Iterations(int32), Salt(bytes), SaltedPassword(bytes)}]`. The **client**
  computes `SaltedPassword = Hi(password, salt, iterations)` (PBKDF2-HMAC); the server derives `StoredKey = H(HMAC(SaltedPassword,
  "Client Key"))` and `ServerKey = HMAC(SaltedPassword, "Server Key")` and stores salt, StoredKey, ServerKey, iterations (SCM:260-282).
  The plaintext password never reaches the server.
- Per-user results (`AlterUserScramCredentialsResult{User, ErrorCode, ErrorMessage}`). Order of results: successful deletions, successful
  upsertions, then errors (hash-map order inside each group; not request order) (SCM:229-257).
- Rules, evaluated deletions first then upsertions:
  - A user name touched **twice in the same request in any combination** (two deletions, deletion + upsertion, two upsertions - the key
    is the *user name*, not user+mechanism) -> `DUPLICATE_RESOURCE`(92) `A user credential cannot be altered twice in the same request`;
    the earlier operation for that user is also cancelled; a third occurrence is ignored (SCM:188-228).
  - metadata.version without SCRAM -> `UNSUPPORTED_VERSION`(35) `The current metadata.version does not support SCRAM`.
  - Empty user name -> `UNACCEPTABLE_CREDENTIAL`(93) `Username must not be empty`; mechanism not 1/2 ->
    `UNSUPPORTED_SASL_MECHANISM`(33) `Unknown SCRAM mechanism` (SCM:311-322).
  - Upsertion iterations `< 4096` -> `UNACCEPTABLE_CREDENTIAL` `Too few iterations`; `> 16384` -> `Too many iterations` (SCM:290-294;
    `MAX_ITERATIONS = 16384`, SCM:58).
  - Deleting a non-existent (user, mechanism) -> `RESOURCE_NOT_FOUND`(91) `Attempt to delete a user credential that does not exist` (SCM:298-309).
  - Salt and salted-password lengths are not validated.
- The credential becomes usable by SCRAM logins as soon as the metadata record is applied (`ScramPublisher` ->
  `CredentialProvider.updateCredential`, per mechanism cache, CredentialProvider.java:36-53).

DescribeUserScramCredentials (handler KA:2444-2455, logic `ScramImage.describe`, ScramImage.java:79-133):
- Auth: `CLUSTER:DESCRIBE`; denied -> whole-request error 31 via `getErrorResponse(CLUSTER_AUTHORIZATION_FAILED.exception)`.
- Empty/null `Users` -> all users that have any credential. Otherwise each requested name: a name repeated in the request ->
  `DUPLICATE_RESOURCE` `Cannot describe SCRAM credentials for the same user twice in a single request: <user>` (for every occurrence's single entry);
  unknown user -> `RESOURCE_NOT_FOUND` `Attempt to describe a user credential that does not exist: <user>`; else
  `CredentialInfos[{Mechanism, Iterations}]` for the user's mechanisms. Never the salt/keys.
- Result order is hash-map order (unspecified).

### 3.7 Re-authentication, session lifetime, SASL after login

- `SessionLifetimeMs` (SaslAuthenticate v1+; v0 has no such field) is computed at login completion (SSA:667-706):
  `credentialExpiration` is null for PLAIN/SCRAM (only delegation-token SCRAM sets it); `connections.max.reauth.ms`
  (default 0, per-mechanism override `listener.name.<l>.<mech>.connections.max.reauth.ms`, `SaslChannelBuilder.java:339-345`) > 0 ->
  lifetime = that value; otherwise **0 (no session limit)**. Recorded: lifetime 0 at v0/v1/v2 (A1). The server records
  `sessionExpirationTimeNanos = now + lifetime` only when a lifetime exists.
- With `connections.max.reauth.ms = 0` (default), a SaslHandshake after login is **not** a re-authentication: it is handled by `KafkaApis` as a
  normal request and answered `ErrorCode ILLEGAL_SASL_STATE`(34) with an empty mechanisms array (KA:1506-1509); SaslAuthenticate
  after login is answered `ILLEGAL_SASL_STATE` with `ErrorMessage "SaslAuthenticate request received after successful authentication"`
  (KA:1511-1516). The connection stays open and the principal unchanged (recorded A7, all versions). The same two answers apply on
  a PLAINTEXT/SSL connection that never authenticates.
- With a session lifetime set (KIP-368): a SaslHandshake on an authenticated channel begins re-authentication when
  `serverSessionExpirationTimeNanos != null` and the previous re-auth started more than 1 s ago (`MIN_REAUTH_INTERVAL_ONE_SECOND_NANOS`,
  KC:68, 540-570; `SS:1010-1012`); otherwise it is processed as a normal (failing) request. During re-auth the SaslServerAuthenticator starts at
  `REAUTH_PROCESS_HANDSHAKE`, handles the handshake, and then the usual SaslAuthenticate exchange; the **mechanism must be the same**
  (else the handshake response is successful but the next token fails with `SASL mechanism 'X' requested by client is not supported for
  re-authentication of mechanism 'Y'`, SSA:656-665), and the **principal must be the same** (`Cannot change principals during
  re-authentication from T.name: T.name`, SSA:642-649). Failure = 58 `... re-authentication ...` and close.
- Once the session has expired, the next request of any type other than the re-auth SaslHandshake **closes the connection** and drops
  in-flight responses (`SS:1015-1020`, KC:641-644). Kafka serves one request per connection at a time (the channel is muted until the
  response is sent, `SS:1041`), so a re-auth handshake only starts between requests; there is no concurrent in-flight request to
  reconcile.
- `connectedClientSupportsReauthentication` = the client sent any SaslAuthenticate at version > 0 (SSA:456-457).

### 3.8 `sasl.enabled.mechanisms` and the order of the mechanism list

`SaslHandshakeResponse.Mechanisms` is `enabledMechanisms`, which the constructor builds as `new ArrayList<>(new HashSet<>(configured))`
(SSA:181): the **iteration order of a `java.util.HashSet<String>`**, not the configured order. Computed with `String.hashCode` and
`HashMap` spreading (`h ^ (h>>>16)`, table size 16): `PLAIN` bucket 0, `SCRAM-SHA-512` bucket 0, `SCRAM-SHA-256` bucket 6; within a
bucket insertion order (the configured order) is kept. Resulting orders:

| Enabled | Mechanisms in the response |
|---|---|
| PLAIN | PLAIN |
| PLAIN, SCRAM-SHA-256 | PLAIN, SCRAM-SHA-256 |
| PLAIN, SCRAM-SHA-512 | PLAIN, SCRAM-SHA-512 (configured order PLAIN first) |
| SCRAM-SHA-256, SCRAM-SHA-512 | SCRAM-SHA-512, SCRAM-SHA-256 |
| PLAIN, SCRAM-SHA-256, SCRAM-SHA-512 | PLAIN, SCRAM-SHA-512, SCRAM-SHA-256 (recorded A11: "PLAIN, SCRAM-SHA-512, SCRAM-SHA-256") |

(If `SCRAM-SHA-512` is configured *before* `PLAIN`, bucket 0 holds `SCRAM-SHA-512, PLAIN`.) Computation script:
`.../scratchpad/phase8/hs.ts`.

### 3.9 Listener security protocols and the default principal builder

- Protocols: `PLAINTEXT`, `SSL`, `SASL_PLAINTEXT`, `SASL_SSL` (`SecurityProtocol`), each listener name maps to one
  (`listener.security.protocol.map`; default maps each protocol name to itself, SocketServerConfigs.java:45-47).
- `principal.builder.class` default is `DefaultKafkaPrincipalBuilder` (`BrokerSecurityConfigs.java:74`). `build(context)` (DKPB:67-87):
  - `PlaintextAuthenticationContext` (PLAINTEXT listener) -> `User:ANONYMOUS` (`KafkaPrincipal.ANONYMOUS`, `KafkaPrincipal.java:45`).
  - `SslAuthenticationContext` (SSL listener) -> `applySslPrincipalMapper(session.getPeerPrincipal())`: `User:` + the first matching
    `ssl.principal.mapping.rules` rule (default `DEFAULT`) applied to the peer certificate's `X500Principal.getName()` (RFC 2253,
    e.g. `CN=alice,OU=dev,O=acme`); no client certificate (`SSLPeerUnverifiedException`, i.e. `ssl.client.auth=none`/`requested` without a cert)
    -> `User:ANONYMOUS`.
  - `SaslAuthenticationContext` (SASL_PLAINTEXT / SASL_SSL) -> for GSSAPI the Kerberos short name, otherwise
    `User:<saslServer.getAuthorizationID()>`: the PLAIN username / the SCRAM username (the `a=` authzid cannot differ, so it equals the user).
    **On SASL_SSL the client certificate is not used for the principal.**
- Authentication always happens before authorization: a SASL listener's connections arrive at the handlers with the SASL user,
  never ANONYMOUS.

---

## 4. TLS (what shows at protocol level)

- The TLS handshake completes in `KafkaChannel.prepare()` (`transportLayer.handshake()`, KC:177-178) before the SASL authenticator or any
  Kafka request is read. A failed handshake is an `IOException`/`SSLException`: the channel is closed, no Kafka bytes exchanged.
- `ssl.client.auth` (`none` default, `requested`, `required`; case-insensitive, `SslClientAuth.java`): `required` -> `setNeedClientAuth(true)`:
  handshake fails without a client certificate (and with an untrusted one); `requested` -> `setWantClientAuth(true)`: a client with no
  certificate completes the handshake and is `User:ANONYMOUS` (SSL listener); a client that presents a certificate not trusted by the
  broker's truststore fails the handshake; `none` -> no request for a certificate, always ANONYMOUS on `SSL`
  (`DefaultSslEngineFactory.java:192-203`; `SslTransportLayer.peerPrincipal` falls back to ANONYMOUS, SslTransportLayer.java:794-802).
- Hostname verification is client-side only (the broker does not verify the peer host).
- On SASL_SSL the SASL login is still required; the certificate only secures the channel (and could feed a custom principal builder).
- Mutual-TLS principal text: `User:CN=alice,OU=dev,O=acme` for the default mapping; `ssl.principal.mapping.rules` rules transform it. This
  is derived from source (DKPB:100-111); **no recording exists** (claims note TLS as not recorded).

## 5. Allowed-hosts / DNS-rebinding guard

Kafka has none on the broker protocol port: the only host-based control is the ACL `host` field matched against the TCP peer address
(1.2). There is no `Host` header concept in the binary protocol, no origin check, and nothing resembling an `allowed-hosts` or rebinding guard in
`SocketServer`/`Selector`/the authenticator. A console (HTTP) host guard is a doze console feature with no Kafka behaviour to copy.

---

## 6. Edge facts re-implementations get wrong

### 6.1 The request pipeline order, before any handler

`SocketServer.Processor.processCompletedReceives` (SS:1002-1058):
1. frame complete on an authenticated (`ready`) channel;
2. `parseRequestHeader` (SS:775-784): unknown key / `apiKey == null` -> exception; version out of range (`!isApiEnabled`) ->
   `UnsupportedVersionException` ("Received request for api with key N (Name) and unsupported version V") or, if the version is within
   the API's range but the API is not served on this listener, `InvalidRequestException` ("Received request for disabled api ...").
   Either way **the exception handler closes the connection and sends nothing** (SS:1049-1057, 925-931). **Exception: ApiVersions** is
   accepted at any version (`ApiKeys.isVersionEnabled`: `if (this == API_VERSIONS) return true`, ApiKeys.java:244-248) and answered
   `UNSUPPORTED_VERSION` at v0 with the ApiVersions key list (`RequestContext.parseRequest`, RequestContext.java:112-117).
   Recorded: `versions-not-served` (claim V2) and `apiversions-unsupported-version`.
3. body parse (`new RequestChannel.Request`, RequestChannel.scala:86): any failure (including the ACL "raw code 0" rule, 1.7)
   -> `InvalidRequestException` -> connection closed, no response.
4. session-expired check (3.7); then the request is queued and the channel muted (one request in flight per connection).
5. only then `KafkaApis.handle` -> the per-API authorization of section 2.

### 6.2 ApiVersions and SaslHandshake ordering on SASL listeners

ApiVersions is optional but, if sent, must be the **first and only** one before SaslHandshake (3.2, claim A8). Both orders
"SaslHandshake first" and "ApiVersions then SaslHandshake" are valid. After login, ApiVersions is served normally by `KafkaApis`.
An ApiVersions with an unsupported/invalid request is answered but does not use up the single allowed ApiVersions slot.
ApiVersions needs no authorization on non-SASL listeners (it never does).

### 6.3 Authenticated but no ACL

A principal with no ACL on anything is denied everywhere except super users and the APIs that do no authorization
(ApiVersions, SaslHandshake/SaslAuthenticate error answers, DescribeCluster, Get/PushTelemetry, the cluster-less
ListGroups/ListTransactions filters returning empty lists). `allow.everyone.if.no.acl.found=true` allows every resource that has *no*
ACL matching its name; `false`/absent denies (1.3).

### 6.4 Throttling and delay

The only security-related delay is `connection.failed.authentication.delay.ms` (default 100 ms) on authentication failure (3.3). Authorization
denials are immediate, not throttled specially (`RHH:47-48` lets ClusterAuthorizationException be throttled like any error; cluster-action
APIs otherwise exempt).

### 6.5 Sizes

- Pre-auth SASL frames: 524288 bytes max (`sasl.server.max.receive.size`), larger -> silent close (3.3). Not applicable to PLAINTEXT/SSL.
- Post-auth: `socket.request.max.bytes` 104857600. A frame length over the limit makes the Selector throw `InvalidReceiveException` -> channel closed.
- A SaslAuthenticate token of 64 KiB is accepted and rejected as bad credentials, not as oversized (A2).

### 6.6 Other precision points

- Re-auth and in-flight requests: none exist (one request per connection at a time, `selector.mute` after each receive, SS:1041).
- A second login attempt on a logged-in channel never changes the principal (A7).
- SCRAM server-final has no error form; failures are SaslAuthenticate errors (3.5).
- SASL error responses on v0 SaslAuthenticate carry `ErrorMessage` too (the field exists from v0); `SessionLifetimeMs` exists from v1.
- The SCRAM channel-binding flags other than `n` are rejected at client-first even though the server advertises no binding (A15).
- `SaslHandshake` v0 is raw-token mode (not recorded; doze serves only v1: `served_apis!` `v(1,1)`).
- `Authorizer` exceptions inside `authorize` (`AuthorizerNotReadyException`) become the API-specific error mapping of the thrown type (not modelled).

---

## 7. Unknowns (source does not settle, or not verified)

- **U1** Exact `ErrorMessage` bytes of cluster-authorization failures (`... needs ALTER permission.` embeds `RequestChannel.Request.toString`): not reproducible; a stable
  stand-in is needed and tests must not pin it.
- **U2** Order of resources/ACLs in DescribeAcls and of `MatchingAcls` in DeleteAcls: `HashMap`/`HashSet` order of Java objects whose `hashCode` is derived from
  enum/string content (stable per JVM but not specified). Clients sort. Not settled.
- **U3** CreateAcls with more than 10000 new distinct ACLs: `BoundedListTooLongException` escapes `AclControlManager.createAcls`; how
  `EventHandlerExceptionInfo.fromInternal` maps it was not read (likely `UNKNOWN_SERVER_ERROR`/`UnknownServerException`). Not read.
- **U4** (resolved, kept for the record) SCRAM misuse cases look like they might hit `IllegalSaslStateException`, but trace to the
  client-first / client-final regex `SaslException`, i.e. 58, matching recording A16 (3.5 item 10).
- **U5** Whether a PLAIN token with invalid UTF-8 yields exactly "Invalid username or password" for every byte pattern (recorded for one pattern, A2).
- **U6** `ErrorMessage` on per-partition `TOPIC_AUTHORIZATION_FAILED` in Produce/Fetch/DeleteRecords responses (PartitionResponse constructor text); not read.
  The recordings of the produce/fetch area may show it.
- **U7** TLS client certificate principal text (`User:` + RFC 2253 DN) and `ssl.principal.mapping.rules`: derived from `X500Principal`
  behaviour, never recorded; mutual TLS and SASL_SSL are outside the recordings.
- **U8** KIP-368 re-authentication is source-derived only; not recorded (no scenario sets `connections.max.reauth.ms`).
- **U9** ACL `host` match for IPv6 clients (`getHostAddress()` format) - derived, not recorded.
- **U10** `Metadata` v12 unauthorized-by-id entry's topic id when the id is unknown to the cache.
- **U11** Whether AuthorizerNotReady can be observed in a one-process design (it cannot unless ACL loading is asynchronous).

## 8. Recorded-data pointers (real Apache Kafka 4.3.1 recordings in the Go repo)

All under `/Users/srini/Code/doze-dev/doze-kafka/conformance/` (not edited; read only):
- `recordings/sasl-plain.json`, `recordings/sasl-scram.json` - the SASL exchange matrix (claims `claims/auth.md` A1-A18, each with step names).
- `recordings/acl-unknown-codes.json` - 77 steps: DescribeAcls/DeleteAcls/CreateAcls with raw codes 0, 99, negative, ANY, MATCH (`claims/api.md` V5).
- `recordings/idempotent-write-from-topic-write.json` - InitProducerId with WRITE on one topic vs no ACL (`claims/api.md` V8).
- `recordings/telemetry-acl.json` - telemetry APIs need no ACL, Metadata denied for the same user (`claims/traffic.md` R2).
- `recordings/versions-not-served.json`, `recordings/api-versions.json`, `recordings/apiversions-unsupported-version.json` - version gating (`claims/api.md` V1, V2).
- Scenario code: `scenarios_sasl.go`, `targets.go` (how the harness sets `acl-default`, super user, SASL users, SCRAM credentials).
- Not recorded, so source-only (see Unknowns): ACL matching beyond the single-topic cases (prefixed/literal/wildcard/DENY/operation implication),
  the per-API denial shapes of section 2 except InitProducerId/Metadata/telemetry, TLS, SASL_SSL, v0 handshake framing, re-authentication, AlterUserScramCredentials error texts.
  Phase 8 should add recordings for these before claiming conformance (the two debt items already name them: `docs/port/debt.md` rows E5 and Phase 8 list).
