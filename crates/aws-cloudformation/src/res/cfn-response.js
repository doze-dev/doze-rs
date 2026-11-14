// The cfn-response module CloudFormation puts beside inline Node.js code (`Code.ZipFile`), written for doze-aws: the same send() with
// the same arguments, over http or https as the response URL says.
exports.SUCCESS = "SUCCESS";
exports.FAILED = "FAILED";

exports.send = function (event, context, responseStatus, responseData, physicalResourceId, noEcho, reason) {
  return new Promise(function (resolve, reject) {
    var body = JSON.stringify({
      Status: responseStatus,
      Reason: reason || "See the details in CloudWatch Log Stream: " + context.logStreamName,
      PhysicalResourceId: physicalResourceId || context.logStreamName,
      StackId: event.StackId,
      RequestId: event.RequestId,
      LogicalResourceId: event.LogicalResourceId,
      NoEcho: noEcho || false,
      Data: responseData,
    });
    var url = new URL(event.ResponseURL);
    var lib = url.protocol === "https:" ? require("https") : require("http");
    var request = lib.request(
      {
        hostname: url.hostname,
        port: url.port,
        path: url.pathname + url.search,
        method: "PUT",
        headers: { "content-type": "", "content-length": Buffer.byteLength(body) },
      },
      function (response) {
        console.log("Status code: " + response.statusCode);
        if (context.done) context.done();
        resolve();
      }
    );
    request.on("error", function (error) {
      console.log("send(..) failed executing https.request(..): " + error);
      if (context.done) context.done();
      reject(error);
    });
    request.write(body);
    request.end();
  });
};
