// CreateAcls past Kafka's 10,000-record bound on one operation (debt I8, kafka-semantics.md U3): raw requests over one plain
// connection, each answer printed as its result codes and messages counted, so that two brokers' outputs can be diffed.
//
//   java -cp '<kafka_2.13-4.4.0>/libs/*' tools/kafka-oracle/AclProbe.java host:port
//
// Kafka needs an authorizer for CreateAcls: start it with `authorizer.class.name=org.apache.kafka.metadata.authorizer.StandardAuthorizer`
// and `super.users=User:ANONYMOUS` added to server.properties. acl-probe-4.4.0.txt is what Kafka 4.4.0 answered.
import org.apache.kafka.common.message.*;
import org.apache.kafka.common.protocol.ApiKeys;
import org.apache.kafka.common.protocol.ApiMessage;
import org.apache.kafka.common.requests.*;

import java.io.*;
import java.net.Socket;
import java.nio.ByteBuffer;
import java.util.*;

public class AclProbe {
    static DataInputStream in;
    static DataOutputStream out;
    static int corr = 0;

    static ApiMessage ask(ApiKeys key, short v, ApiMessage data) throws IOException {
        RequestHeader h = new RequestHeader(key, v, "probe", ++corr);
        ByteBuffer b = RequestUtils.serialize(h.data(), h.headerVersion(), data, v);
        out.writeInt(b.remaining());
        out.write(b.array(), b.arrayOffset() + b.position(), b.remaining());
        out.flush();
        int n = in.readInt();
        byte[] r = new byte[n];
        in.readFully(r);
        return AbstractResponse.parseResponse(ByteBuffer.wrap(r), h).data();
    }

    /// `n` ACLs on topics named `prefix-0` to `prefix-(n-1)`: READ allowed to User:probe from any host, literal.
    static CreateAclsRequestData create(String prefix, int n) {
        CreateAclsRequestData d = new CreateAclsRequestData();
        for (int i = 0; i < n; i++) {
            d.creations().add(new CreateAclsRequestData.AclCreation()
                .setResourceType((byte) 2).setResourceName(prefix + "-" + i).setResourcePatternType((byte) 3)
                .setPrincipal("User:probe").setHost("*").setOperation((byte) 3).setPermissionType((byte) 3));
        }
        return d;
    }

    static void tell(String label, CreateAclsResponseData r) {
        Map<String, Integer> seen = new TreeMap<>();
        for (CreateAclsResponseData.AclCreationResult x : r.results()) {
            seen.merge(x.errorCode() + " " + x.errorMessage(), 1, Integer::sum);
        }
        System.out.println(label + ": " + r.results().size() + " results " + seen);
    }

    static int count() throws IOException {
        DescribeAclsRequestData d = new DescribeAclsRequestData()
            .setResourceTypeFilter((byte) 1).setPatternTypeFilter((byte) 1)
            .setResourceNameFilter(null).setPrincipalFilter(null).setHostFilter(null)
            .setOperation((byte) 1).setPermissionType((byte) 1);
        DescribeAclsResponseData r = (DescribeAclsResponseData) ask(ApiKeys.DESCRIBE_ACLS, (short) 3, d);
        return r.resources().stream().mapToInt(x -> x.acls().size()).sum();
    }

    public static void main(String[] args) throws Exception {
        String[] hp = args[0].split(":");
        Socket sock = new Socket(hp[0], Integer.parseInt(hp[1]));
        in = new DataInputStream(new BufferedInputStream(sock.getInputStream()));
        out = new DataOutputStream(new BufferedOutputStream(sock.getOutputStream()));
        short v = 3;
        tell("10001 new", (CreateAclsResponseData) ask(ApiKeys.CREATE_ACLS, v, create("over", 10001)));
        System.out.println("acls now: " + count());
        tell("10000 new", (CreateAclsResponseData) ask(ApiKeys.CREATE_ACLS, v, create("at", 10000)));
        System.out.println("acls now: " + count());
        tell("the same 10000 again plus 1 new", (CreateAclsResponseData) ask(ApiKeys.CREATE_ACLS, v, plusOne()));
        System.out.println("acls now: " + count());
        tell("10001 with 2 the same", (CreateAclsResponseData) ask(ApiKeys.CREATE_ACLS, v, twice()));
        System.out.println("acls now: " + count());
        tell("10001 new and 1 invalid", (CreateAclsResponseData) ask(ApiKeys.CREATE_ACLS, v, withInvalid()));
        System.out.println("acls now: " + count());
        sock.close();
    }

    static CreateAclsRequestData plusOne() {
        CreateAclsRequestData d = create("at", 10000);
        d.creations().addAll(create("one", 1).creations());
        return d;
    }

    /// 10,001 new ACLs and one whose principal has no type (an invalid one, first).
    static CreateAclsRequestData withInvalid() {
        CreateAclsRequestData d = create("bad", 1);
        d.creations().get(0).setPrincipal("probe");
        d.creations().addAll(create("more", 10001).creations());
        return d;
    }

    /// 10,001 creations of which two are the same ACL: 10,000 distinct.
    static CreateAclsRequestData twice() {
        CreateAclsRequestData d = create("dup", 10000);
        d.creations().add(create("dup", 1).creations().get(0));
        return d;
    }
}
