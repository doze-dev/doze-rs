# The cfn-response module CloudFormation puts beside inline Python code (`Code.ZipFile`), written for doze-aws: the same send() with
# the same arguments, using only the standard library.
import json
import urllib.request

SUCCESS = "SUCCESS"
FAILED = "FAILED"


def send(event, context, responseStatus, responseData, physicalResourceId=None, noEcho=False, reason=None):
    body = {
        "Status": responseStatus,
        "Reason": reason or "See the details in CloudWatch Log Stream: {}".format(context.log_stream_name),
        "PhysicalResourceId": physicalResourceId or context.log_stream_name,
        "StackId": event["StackId"],
        "RequestId": event["RequestId"],
        "LogicalResourceId": event["LogicalResourceId"],
        "NoEcho": noEcho,
        "Data": responseData,
    }
    data = json.dumps(body).encode("utf-8")
    request = urllib.request.Request(
        event["ResponseURL"], data=data, method="PUT", headers={"content-type": "", "content-length": str(len(data))}
    )
    try:
        with urllib.request.urlopen(request) as response:
            print("Status code:", response.status)
    except Exception as e:
        print("send(..) failed executing http.request(..):", e)
