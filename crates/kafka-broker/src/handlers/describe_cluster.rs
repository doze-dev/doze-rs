//! DescribeCluster (KIP-700, KIP-919): the cluster id, the controller and the brokers, as Metadata says them.

use kafka_storage::fs::Fs;
use kafka_wire::api::Response;
use kafka_wire::messages::describe_cluster_response::DescribeClusterBroker;
use kafka_wire::messages::{BrokerId, DescribeClusterRequest, DescribeClusterResponse};
use kafka_wire::{ResponseError, StrBytes};

use crate::{Broker, Caller, Listener, Outcome};

/// The endpoint type of a broker, the only one this node is (`EndpointType.BROKER`).
const BROKER: i8 = 1;
/// The endpoint type of a KRaft controller (`EndpointType.CONTROLLER`), which this node is not.
const CONTROLLER: i8 = 2;

impl<F: Fs> Broker<F> {
    /// This broker is the whole cluster and its own controller. No ACL decides whether the answer is given
    /// (`AuthHelper.computeDescribeClusterResponse` asks none): the one right that matters is DESCRIBE on the cluster, and
    /// only to fill the authorized operations when they were asked for, `0` when it is denied and the schema's default
    /// (`i32::MIN`) when they were not asked for.
    ///
    /// The endpoint type is checked first, as Kafka does: a type that is neither BROKER nor CONTROLLER is
    /// UNSUPPORTED_ENDPOINT_TYPE, and CONTROLLER, which this endpoint is not, is MISMATCHED_ENDPOINT_TYPE (INVALID_REQUEST
    /// to version 0, which cannot name a type). Brokers fenced (v2) are never listed: this one is never fenced.
    pub(crate) fn describe_cluster(
        &self,
        at: &Listener,
        caller: &Caller,
        req: &DescribeClusterRequest,
        version: i16,
    ) -> Outcome {
        let mut resp = DescribeClusterResponse::default();
        match req.endpoint_type {
            BROKER => self.fill_cluster(at, caller, req, &mut resp),
            CONTROLLER => {
                let error = if version == 0 {
                    ResponseError::InvalidRequest
                } else {
                    ResponseError::MismatchedEndpointType
                };
                resp.error_code = error.code();
                resp.error_message = Some(StrBytes::from_static_str(
                    "The request was sent to an endpoint of type BROKER, but we wanted an endpoint of type CONTROLLER",
                ));
            }
            other => {
                resp.error_code = ResponseError::UnsupportedEndpointType.code();
                resp.error_message = Some(StrBytes::from_string(format!(
                    "Unsupported endpoint type {other}"
                )));
            }
        }
        Outcome::Respond(Response::DescribeCluster(resp))
    }

    fn fill_cluster(
        &self,
        at: &Listener,
        caller: &Caller,
        req: &DescribeClusterRequest,
        resp: &mut DescribeClusterResponse,
    ) {
        let mut me = DescribeClusterBroker::default();
        me.broker_id = BrokerId(self.config.node_id);
        me.host = StrBytes::from_string(at.advertised_host.clone());
        me.port = at.advertised_port;
        resp.brokers.push(me);
        resp.cluster_id = StrBytes::from_string(self.config.cluster_id.clone());
        resp.controller_id = BrokerId(self.config.node_id);
        if req.include_cluster_authorized_operations {
            resp.cluster_authorized_operations = self.cluster_operations(caller);
        }
    }
}
