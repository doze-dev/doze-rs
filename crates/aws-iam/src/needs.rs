//! What a call to IAM or STS asks to do, for the authorizer; and the credential behind a key, for the gateway's signature check.

use aws_peers::Need;
use aws_wire::service::{Call, Caller};

use crate::core::{Core, Which};
use crate::model::Kind;
use crate::req::Req;
use crate::session::Session;

impl Core {
    /// Every thing a call to IAM or STS asks to do. `GetCallerIdentity` and `GetSessionToken` are never asked about: AWS lets any
    /// caller see who they are, and a session token is made from what a user already has.
    pub(crate) fn needs_of(&self, which: Which, call: &Call) -> Vec<Need> {
        let r = Req::new(call);
        match which {
            Which::Sts => self.sts_needs(call, &r),
            Which::Iam => {
                let Some(op) = aws_model_iam::Operation::from_index(call.op) else {
                    return Vec::new();
                };
                let mut need = Need::new(
                    format!("iam:{op:?}"),
                    self.iam_resource(op, &r, &call.caller),
                );
                if let Some(arn) = r.opt("PolicyArn") {
                    need = need.with("iam:PolicyARN", vec![arn.to_owned()]);
                }
                if let Some(arn) = r.opt("PermissionsBoundary") {
                    need = need.with("iam:PermissionsBoundary", vec![arn.to_owned()]);
                }
                vec![need]
            }
        }
    }

    fn sts_needs(&self, call: &Call, r: &Req<'_>) -> Vec<Need> {
        use aws_model_sts::Operation as O;
        let Some(op) = O::from_index(call.op) else {
            return Vec::new();
        };
        match op {
            O::GetCallerIdentity
            | O::GetSessionToken
            | O::AssumeRoleWithWebIdentity
            | O::AssumeRoleWithSAML => Vec::new(),
            O::AssumeRole => {
                let arn = r.s("RoleArn");
                let trust = arn
                    .rsplit('/')
                    .next()
                    .and_then(|name| self.entity(Kind::Role, name).ok())
                    .and_then(|role| role.trust.clone());
                let mut need = Need::new("sts:AssumeRole", arn).with_policy(trust).gated();
                // The external ID a caller names is what a trust policy's `sts:ExternalId` condition reads (the confused-deputy guard).
                if let Some(external) = r.opt("ExternalId") {
                    need = need.with("sts:ExternalId", vec![external.into()]);
                }
                // A good MFA code is what a trust policy's `aws:MultiFactorAuthPresent` asks about (the call spends it).
                if let Some(serial) = r.opt("SerialNumber") {
                    let user = self.identify(r).arn;
                    let user = user
                        .split_once(":user/")
                        .and_then(|(_, n)| n.rsplit('/').next());
                    if self.mfa_step(user, serial, r.s("TokenCode")).is_ok() {
                        need = need
                            .with("aws:MultiFactorAuthPresent", vec!["true".into()])
                            .with("aws:MultiFactorAuthAge", vec!["0".into()]);
                    }
                }
                vec![need]
            }
            other => vec![Need::new(format!("sts:{other:?}"), "*")],
        }
    }

    /// The ARN of what an IAM operation is on: the entity, policy or profile it names, else `*`.
    fn iam_resource(&self, op: aws_model_iam::Operation, r: &Req<'_>, caller: &Caller) -> String {
        use aws_model_iam::Operation as O;
        let account = &caller.account_id;
        let entity = |kind: Kind, name: &str| {
            self.entity(kind, name).map_or_else(
                |_| {
                    let path = r.opt("Path").unwrap_or("/");
                    format!("arn:aws:iam::{account}:{}{path}{name}", kind.word())
                },
                |e| e.arn.clone(),
            )
        };
        let group_first = matches!(op, O::AddUserToGroup | O::RemoveUserFromGroup);
        if group_first && let Some(name) = r.opt("GroupName") {
            return entity(Kind::Group, name);
        }
        if let Some(name) = r.opt("UserName") {
            return entity(Kind::User, name);
        }
        if let Some(name) = r.opt("GroupName") {
            return entity(Kind::Group, name);
        }
        if let Some(name) = r.opt("RoleName") {
            return entity(Kind::Role, name);
        }
        // `Arn` names what a last-accessed call is about.
        for member in [
            "PolicyArn",
            "OpenIDConnectProviderArn",
            "SAMLProviderArn",
            "Arn",
        ] {
            if let Some(arn) = r.opt(member) {
                return arn.to_owned();
            }
        }
        if let Some(arn) = self.held_resource(op, r, account) {
            return arn;
        }
        if let Some(name) = r.opt("InstanceProfileName") {
            let path = r.opt("Path").unwrap_or("/");
            return self.profiles.get(name).map_or_else(
                || format!("arn:aws:iam::{account}:instance-profile{path}{name}"),
                |p| p.arn.clone(),
            );
        }
        // A key made or listed without a user is the caller's own.
        if matches!(
            op,
            O::CreateAccessKey
                | O::ListAccessKeys
                | O::ChangePassword
                | O::ListMFADevices
                | O::DeactivateMFADevice
                | O::UploadSigningCertificate
                | O::ListSigningCertificates
                | O::UpdateSigningCertificate
                | O::DeleteSigningCertificate
                | O::ListSSHPublicKeys
                | O::ListServiceSpecificCredentials
                | O::ResetServiceSpecificCredential
                | O::UpdateServiceSpecificCredential
                | O::DeleteServiceSpecificCredential
        ) && let Some(arn) = self.caller_arn(caller)
        {
            return arn;
        }
        "*".to_owned()
    }
}

impl Core {
    /// The ARN of a virtual MFA device or a server certificate a call is on. A virtual MFA device's serial number is its ARN; the
    /// calls on a user's device are on the user, and answered before this.
    fn held_resource(
        &self,
        op: aws_model_iam::Operation,
        r: &Req<'_>,
        account: &str,
    ) -> Option<String> {
        use aws_model_iam::Operation as O;
        let path = r.opt("Path").unwrap_or("/");
        if matches!(
            op,
            O::DeleteVirtualMFADevice | O::TagMFADevice | O::UntagMFADevice | O::ListMFADeviceTags
        ) && let Some(serial) = r.opt("SerialNumber")
        {
            return Some(serial.to_owned());
        }
        if let Some(name) = r.opt("VirtualMFADeviceName") {
            return Some(format!("arn:aws:iam::{account}:mfa{path}{name}"));
        }
        let name = r.opt("ServerCertificateName")?;
        Some(self.held.certs.get(name).map_or_else(
            || format!("arn:aws:iam::{account}:server-certificate{path}{name}"),
            |c| c.arn.clone(),
        ))
    }

    /// The credential behind an access key id, for checking a signature: the secret of an access key IAM issued (while it is
    /// active), or the one a role session derives (with the token the request must carry and the time it runs out).
    pub(crate) fn credential(
        &self,
        access_key_id: &str,
        token: Option<&str>,
    ) -> Option<aws_auth::Credential> {
        if access_key_id.starts_with("ASIA") {
            let key = self.account().signing_key;
            let session = Session::read(token?, &key)?;
            if session.access_key_id(&key) != access_key_id {
                return None;
            }
            return Some(aws_auth::Credential {
                secret: aws_auth::Secret::new(session.secret_access_key(&key)),
                session_token: token.map(str::to_owned),
                expires_at: Some(session.expires_ms / 1000),
            });
        }
        let issued = self.keys.get(access_key_id).filter(|k| k.active)?;
        Some(aws_auth::Credential {
            secret: aws_auth::Secret::new(issued.secret.clone()),
            session_token: None,
            expires_at: None,
        })
    }
}
