//! What a model says about its service as a whole: how it is named, signed, targeted and spoken to. Generated per model as
//! `meta::META`; the wire layer reads it to route and to word errors.

/// A wire protocol a service speaks.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum Protocol {
    /// `awsJson1_0`: JSON in a POST to `/`, the operation in `X-Amz-Target`.
    AwsJson10,
    /// `awsJson1_1`: as 1.0, with a different content type.
    AwsJson11,
    /// `awsQuery`: a form-encoded `Action`, XML out.
    AwsQuery,
    /// `restJson1`: HTTP method and path pick the operation, JSON bodies.
    RestJson1,
    /// `restXml`: as restJson1 with XML bodies (S3, CloudFront).
    RestXml,
    /// `rpcv2Cbor`: CBOR bodies under `/service/<Service>/operation/<Operation>`.
    Rpcv2Cbor,
}

/// One service's identity.
#[derive(Clone, Copy, Debug)]
pub struct ServiceMeta {
    /// The service shape's name (`AmazonSQS`, `DynamoDB_20120810`): the prefix of `X-Amz-Target` for the JSON protocols.
    pub shape_name: &'static str,
    /// The Smithy namespace (`com.amazonaws.sqs`): the qualifier of `__type` in JSON errors.
    pub namespace: &'static str,
    /// The service name in a SigV4 credential scope (`sqs`, `monitoring`).
    pub signing_name: &'static str,
    /// The SDK id (`SQS`).
    pub sdk_id: &'static str,
    /// The API version (`2012-11-05`).
    pub version: &'static str,
    /// The protocols it speaks, in the model's preference order.
    pub protocols: &'static [Protocol],
    /// `awsQueryCompatible`: JSON errors also say, in `x-amzn-query-error`, the code the Query protocol would.
    pub query_compatible: bool,
    /// The `xmlNamespace` of its XML bodies, if any.
    pub xml_namespace: Option<&'static str>,
}

impl ServiceMeta {
    /// Whether the service speaks `protocol`.
    #[must_use]
    pub fn speaks(&self, protocol: Protocol) -> bool {
        self.protocols.contains(&protocol)
    }
}
