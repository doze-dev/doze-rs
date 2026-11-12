//! From an operation to the code that does it: every operation of the model is either run here or refused by name, and the dispatch is
//! exhaustive over the model.

use aws_model_lambda::Operation;
use aws_wire::ApiError;
use aws_wire::service::Call;

use crate::core::{Core, Out, Staged, built};
use crate::req::Req;

impl Core {
    pub(crate) fn dispatch(&mut self, call: &Call, staged: &Staged) -> Result<Out, ApiError> {
        let Some(op) = Operation::from_index(call.op).filter(|op| built(*op)) else {
            return Err(crate::errors::refused("This operation"));
        };
        let r = Req::new(call);
        match op {
            Operation::CreateFunction => self.create_function(&r, staged),
            Operation::UpdateFunctionCode => self.update_function_code(&r, staged),
            Operation::UpdateFunctionConfiguration => {
                self.update_function_configuration(&r, staged)
            }
            Operation::GetFunction => self.get_function(&r),
            Operation::GetFunctionConfiguration => self.get_function_configuration(&r),
            Operation::ListFunctions => self.list_functions(&r),
            Operation::DeleteFunction => self.delete_function(&r),
            Operation::PublishVersion => self.publish_version(&r),
            Operation::ListVersionsByFunction => self.list_versions(&r),
            other => self.dispatch_more(other, &r, staged),
        }
    }

    fn dispatch_more(&mut self, op: Operation, r: &Req, staged: &Staged) -> Result<Out, ApiError> {
        match op {
            Operation::CreateAlias => self.create_alias(r),
            Operation::GetAlias => self.get_alias(r),
            Operation::UpdateAlias => self.update_alias(r),
            Operation::DeleteAlias => self.delete_alias(r),
            Operation::ListAliases => self.list_aliases(r),
            Operation::AddPermission => self.add_permission(r),
            Operation::RemovePermission => self.remove_permission(r),
            Operation::GetPolicy => self.get_policy(r),
            Operation::PutResourcePolicy => self.put_resource_policy(r),
            Operation::GetResourcePolicy => self.get_resource_policy(r),
            Operation::DeleteResourcePolicy => self.delete_resource_policy(r),
            Operation::CreateFunctionUrlConfig => self.create_url(r),
            Operation::GetFunctionUrlConfig => self.get_url(r),
            Operation::UpdateFunctionUrlConfig => self.update_url(r),
            Operation::DeleteFunctionUrlConfig => self.delete_url(r),
            Operation::ListFunctionUrlConfigs => self.list_urls(r),
            Operation::PublishLayerVersion => self.publish_layer(r, staged),
            other => self.dispatch_layers(other, r, staged),
        }
    }

    fn dispatch_layers(
        &mut self,
        op: Operation,
        r: &Req,
        staged: &Staged,
    ) -> Result<Out, ApiError> {
        match op {
            Operation::GetLayerVersion => self.get_layer_version(r),
            Operation::GetLayerVersionByArn => self.get_layer_version_by_arn(r),
            Operation::ListLayers => self.list_layers(r),
            Operation::ListLayerVersions => self.list_layer_versions(r),
            Operation::DeleteLayerVersion => self.delete_layer_version(r),
            Operation::AddLayerVersionPermission => self.add_layer_permission(r),
            Operation::RemoveLayerVersionPermission => self.remove_layer_permission(r),
            Operation::GetLayerVersionPolicy => self.get_layer_policy(r),
            Operation::CreateEventSourceMapping => self.create_mapping(r, staged),
            Operation::GetEventSourceMapping => self.get_mapping(r),
            Operation::UpdateEventSourceMapping => self.update_mapping(r),
            Operation::DeleteEventSourceMapping => self.delete_mapping(r),
            Operation::ListEventSourceMappings => self.list_mappings(r),
            other => self.dispatch_settings(other, r),
        }
    }

    fn dispatch_settings(&mut self, op: Operation, r: &Req) -> Result<Out, ApiError> {
        match op {
            Operation::PutFunctionConcurrency => self.put_concurrency(r),
            Operation::GetFunctionConcurrency => self.get_concurrency(r),
            Operation::DeleteFunctionConcurrency => self.delete_concurrency(r),
            Operation::PutProvisionedConcurrencyConfig => self.put_provisioned(r),
            Operation::GetProvisionedConcurrencyConfig => self.get_provisioned(r),
            Operation::DeleteProvisionedConcurrencyConfig => self.delete_provisioned(r),
            Operation::ListProvisionedConcurrencyConfigs => self.list_provisioned(r),
            Operation::PutFunctionEventInvokeConfig
            | Operation::UpdateFunctionEventInvokeConfig => {
                self.put_event_invoke(r, op == Operation::UpdateFunctionEventInvokeConfig)
            }
            Operation::GetFunctionEventInvokeConfig => self.get_event_invoke(r),
            Operation::DeleteFunctionEventInvokeConfig => self.delete_event_invoke(r),
            Operation::ListFunctionEventInvokeConfigs => self.list_event_invoke(r),
            Operation::TagResource => self.tag_resource(r),
            Operation::UntagResource => self.untag_resource(r),
            Operation::ListTags => self.list_tags(r),
            Operation::GetAccountSettings => Ok(self.account_settings(r)),
            other => self.dispatch_stored(other, r),
        }
    }

    fn dispatch_stored(&mut self, op: Operation, r: &Req) -> Result<Out, ApiError> {
        match op {
            Operation::PutRuntimeManagementConfig => self.put_extra(r, "RuntimeManagementConfig"),
            Operation::GetRuntimeManagementConfig => self.get_runtime_management(r),
            Operation::PutFunctionRecursionConfig => self.put_extra(r, "RecursiveLoop"),
            Operation::GetFunctionRecursionConfig => self.get_recursion(r),
            Operation::CreateCodeSigningConfig => self.create_signing(r),
            Operation::GetCodeSigningConfig => self.get_signing(r),
            Operation::UpdateCodeSigningConfig => self.update_signing(r),
            Operation::DeleteCodeSigningConfig => self.delete_signing(r),
            Operation::ListCodeSigningConfigs => self.list_signing(r),
            Operation::PutFunctionCodeSigningConfig => self.put_function_signing(r),
            Operation::GetFunctionCodeSigningConfig => self.get_function_signing(r),
            Operation::DeleteFunctionCodeSigningConfig => self.delete_function_signing(r),
            Operation::ListFunctionsByCodeSigningConfig => self.functions_by_signing(r),
            _ => Err(crate::errors::refused("This operation")),
        }
    }
}
