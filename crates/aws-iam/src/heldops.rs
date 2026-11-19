//! The dispatch of the operations on what IAM holds beside its identities (`held`): passwords, MFA devices, certificates, the
//! other user credentials, the credential report and last-accessed data. The rest of the dispatch is in `ops`.

use aws_model_iam::Operation as Op;
use aws_wire::ApiError;
use serde_json::Value;

use crate::core::Core;
use crate::req::Req;

impl Core {
    pub(crate) fn run_held(&mut self, op: Op, r: &Req<'_>) -> Result<Value, ApiError> {
        match op {
            Op::ChangePassword => self.change_password(r),
            Op::SetSecurityTokenServicePreferences => Ok(self.set_sts_preferences(r)),
            Op::CreateVirtualMFADevice => self.create_virtual_mfa_device(r),
            Op::EnableMFADevice => self.enable_mfa_device(r),
            Op::ResyncMFADevice => self.resync_mfa_device(r),
            Op::DeactivateMFADevice => self.deactivate_mfa_device(r),
            Op::DeleteVirtualMFADevice => self.delete_virtual_mfa_device(r),
            Op::GetMFADevice => self.get_mfa_device(r),
            Op::ListMFADevices => self.list_mfa_devices(r),
            Op::ListVirtualMFADevices => self.list_virtual_mfa_devices(r),
            Op::TagMFADevice => self.tag_mfa_device(r),
            Op::UntagMFADevice => self.untag_mfa_device(r),
            Op::ListMFADeviceTags => self.list_mfa_device_tags(r),
            Op::GenerateCredentialReport => {
                Ok(self.generate_credential_report(&r.call.caller.account_id))
            }
            Op::GetCredentialReport => self.get_credential_report(),
            Op::GenerateServiceLastAccessedDetails => self.generate_service_last_accessed(r),
            Op::GetServiceLastAccessedDetails => self.get_service_last_accessed(r),
            Op::GetServiceLastAccessedDetailsWithEntities => {
                self.get_service_last_accessed_with_entities(r)
            }
            Op::ListPoliciesGrantingServiceAccess => self.list_policies_granting_service_access(r),
            _ => self.run_certificates(op, r),
        }
    }

    fn run_certificates(&mut self, op: Op, r: &Req<'_>) -> Result<Value, ApiError> {
        match op {
            Op::UploadServerCertificate => self.upload_server_certificate(r),
            Op::GetServerCertificate => self.get_server_certificate(r),
            Op::ListServerCertificates => self.list_server_certificates(r),
            Op::UpdateServerCertificate => self.update_server_certificate(r),
            Op::DeleteServerCertificate => self.delete_server_certificate(r),
            Op::TagServerCertificate => self.tag_server_certificate(r),
            Op::UntagServerCertificate => self.untag_server_certificate(r),
            Op::ListServerCertificateTags => self.list_server_certificate_tags(r),
            Op::UploadSigningCertificate => self.upload_signing_certificate(r),
            Op::ListSigningCertificates => self.list_signing_certificates(r),
            Op::UpdateSigningCertificate => self.update_signing_certificate(r),
            Op::DeleteSigningCertificate => self.delete_signing_certificate(r),
            Op::UploadSSHPublicKey => self.upload_ssh_public_key(r),
            Op::GetSSHPublicKey => self.get_ssh_public_key(r),
            Op::ListSSHPublicKeys => self.list_ssh_public_keys(r),
            Op::UpdateSSHPublicKey => self.update_ssh_public_key(r),
            Op::DeleteSSHPublicKey => self.delete_ssh_public_key(r),
            Op::CreateServiceSpecificCredential => self.create_service_specific_credential(r),
            Op::ListServiceSpecificCredentials => self.list_service_specific_credentials(r),
            Op::ResetServiceSpecificCredential => self.reset_service_specific_credential(r),
            Op::UpdateServiceSpecificCredential => self.update_service_specific_credential(r),
            Op::DeleteServiceSpecificCredential => self.delete_service_specific_credential(r),
            _ => Err(ApiError::new(
                501,
                "NotImplemented",
                "This operation is not built in doze-aws.",
            )),
        }
    }
}
