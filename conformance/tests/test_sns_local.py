"""SNS mobile push and SMS, through boto3, against doze-aws only.

These are NOT compared with AWS and have no recording: a platform application needs a push credential AWS validates (a real
Firebase key or an Apple certificate), and a text message costs money and goes to a real phone. doze-aws accepts and stores them
(decision D1: nothing is sent), so what these hold is that boto3 can drive the calls and the stored state comes back.
Against real AWS they are skipped. They count in no fidelity score.
"""

import pytest


@pytest.fixture
def local_only(target):
    if target.name == "aws":
        pytest.skip("needs a real push credential and sends real messages; doze-aws only")


def test_an_application_and_a_device_are_stored(client, names, cleanup, local_only):
    sns = client("sns")
    app = sns.create_platform_application(
        Name=names("app"), Platform="GCM", Attributes={"PlatformCredential": "a-key"})["PlatformApplicationArn"]
    cleanup(sns.delete_platform_application, PlatformApplicationArn=app)
    got = sns.get_platform_application_attributes(PlatformApplicationArn=app)["Attributes"]
    assert "PlatformCredential" not in got, "the credential is never answered"
    endpoint = sns.create_platform_endpoint(
        PlatformApplicationArn=app, Token="device-token", CustomUserData="me")["EndpointArn"]
    assert sns.get_endpoint_attributes(EndpointArn=endpoint)["Attributes"] == {
        "CustomUserData": "me", "Enabled": "true", "Token": "device-token"}
    listed = sns.list_endpoints_by_platform_application(PlatformApplicationArn=app)["Endpoints"]
    assert [e["EndpointArn"] for e in listed] == [endpoint]
    assert "MessageId" in sns.publish(TargetArn=endpoint, Message="hello device")
    sns.set_endpoint_attributes(EndpointArn=endpoint, Attributes={"Enabled": "false"})
    with pytest.raises(Exception) as refused:
        sns.publish(TargetArn=endpoint, Message="hello device")
    assert "EndpointDisabled" in str(refused.value)
    assert [a["PlatformApplicationArn"] for a in sns.list_platform_applications()["PlatformApplications"]] == [app]
    sns.delete_endpoint(EndpointArn=endpoint)


def test_an_application_endpoint_can_be_subscribed_to_a_topic(client, names, cleanup, local_only):
    sns = client("sns")
    topic = sns.create_topic(Name=names("topic"))["TopicArn"]
    cleanup(sns.delete_topic, TopicArn=topic)
    app = sns.create_platform_application(
        Name=names("app"), Platform="APNS_SANDBOX", Attributes={"PlatformCredential": "a-key"})["PlatformApplicationArn"]
    cleanup(sns.delete_platform_application, PlatformApplicationArn=app)
    endpoint = sns.create_platform_endpoint(PlatformApplicationArn=app, Token="t")["EndpointArn"]
    sub = sns.subscribe(TopicArn=topic, Protocol="application", Endpoint=endpoint)["SubscriptionArn"]
    assert sub.startswith(topic + ":")
    assert "MessageId" in sns.publish(TopicArn=topic, Message="to everyone")


def test_a_text_message_is_accepted_and_sent_nowhere(client, local_only):
    sns = client("sns")
    assert "MessageId" in sns.publish(PhoneNumber="+14155552671", Message="hello")
    sns.set_sms_attributes(attributes={"DefaultSMSType": "Transactional"})
    assert sns.get_sms_attributes(attributes=["DefaultSMSType"])["attributes"] == {"DefaultSMSType": "Transactional"}
    assert sns.check_if_phone_number_is_opted_out(phoneNumber="+14155552671")["isOptedOut"] is False
    assert sns.list_phone_numbers_opted_out()["phoneNumbers"] == []
    sns.opt_in_phone_number(phoneNumber="+14155552671")


def test_the_sms_sandbox_is_refused_by_name(client, local_only):
    sns = client("sns")
    with pytest.raises(Exception) as refused:
        sns.get_sms_sandbox_account_status()
    assert "NotImplemented" in str(refused.value) or "501" in str(refused.value)
