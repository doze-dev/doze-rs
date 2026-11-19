//! The account itself: aliases, the summary, the password policy, console passwords (login profiles), and service-linked roles.

use std::collections::{BTreeMap, BTreeSet};

use aws_wire::ApiError;
use serde_json::{Map, Value, json};

use crate::core::Core;
use crate::entity::{arn_of, new_id};
use crate::errors::{conflict, exists, no_such};
use crate::held::Pref;
use crate::model::{Entity, Kind};

const TOKEN_VERSION: &str = "global-endpoint-token-version";
use crate::req::{Req, page, secs};

/// The password-policy members, as the request gives them and as `GetAccountPasswordPolicy` shows them (with these defaults).
const POLICY_DEFAULTS: [(&str, i64); 3] = [
    ("MinimumPasswordLength", 6),
    ("MaxPasswordAge", 0),
    ("PasswordReusePrevention", 0),
];
const POLICY_FLAGS: [&str; 6] = [
    "RequireSymbols",
    "RequireNumbers",
    "RequireUppercaseCharacters",
    "RequireLowercaseCharacters",
    "AllowUsersToChangePassword",
    "HardExpiry",
];

impl Core {
    pub(crate) fn create_account_alias(&mut self, r: &Req<'_>) -> Result<Value, ApiError> {
        let alias = r.s("AccountAlias").to_owned();
        let Some(a) = self.account_mut() else {
            return Ok(Value::Null);
        };
        if !a.aliases.is_empty() {
            return Err(exists("Account alias", &alias));
        }
        a.aliases.push(alias);
        Ok(Value::Null)
    }

    pub(crate) fn delete_account_alias(&mut self, r: &Req<'_>) -> Result<Value, ApiError> {
        let alias = r.s("AccountAlias").to_owned();
        let Some(a) = self.account_mut() else {
            return Ok(Value::Null);
        };
        if !a.aliases.contains(&alias) {
            return Err(no_such("account alias", &alias));
        }
        a.aliases.retain(|x| *x != alias);
        Ok(Value::Null)
    }

    pub(crate) fn list_account_aliases(&self, r: &Req<'_>) -> Result<Value, ApiError> {
        page(
            r,
            self.account().aliases.iter().map(|a| json!(a)).collect(),
            "AccountAliases",
        )
    }

    pub(crate) fn get_account_summary(&self) -> Value {
        let count = |kind: Kind| {
            self.entities
                .with_prefix(&format!("{}/", kind.word()))
                .count()
        };
        let mfa = self.held.mfa.len();
        let mfa_in_use = self
            .held
            .mfa
            .iter()
            .filter(|(_, d)| d.user.is_some())
            .count();
        let certs = self.held.certs.len();
        json!({"SummaryMap": {
            "Users": count(Kind::User), "UsersQuota": 5000, "Groups": count(Kind::Group), "GroupsQuota": 300,
            "Roles": count(Kind::Role), "RolesQuota": 1000, "Policies": self.policies.iter().filter(|(_, p)| !p.aws_managed).count(),
            "PoliciesQuota": 1500, "InstanceProfiles": self.profiles.len(), "InstanceProfilesQuota": 1000,
            "AccessKeysPerUserQuota": 2, "AttachedPoliciesPerRoleQuota": 10, "AttachedPoliciesPerUserQuota": 10,
            "AttachedPoliciesPerGroupQuota": 10, "MFADevices": mfa, "MFADevicesInUse": mfa_in_use, "AccountMFAEnabled": 0,
            "ServerCertificates": certs, "ServerCertificatesQuota": crate::certs::SERVER_CERTS, "SigningCertificatesPerUserQuota": 2,
            "PolicyVersionsInUse": self.policies.iter().map(|(_, p)| p.versions.len()).sum::<usize>(), "PolicyVersionsInUseQuota": 10000,
            "VersionsPerPolicyQuota": 5, "GlobalEndpointTokenVersion": self.token_version(),
        }})
    }

    /// `SetSecurityTokenServicePreferences`: the version of the tokens the global STS endpoint gives. Kept and reported in
    /// `GetAccountSummary`; doze-aws has one endpoint and one token form, valid in every region.
    pub(crate) fn set_sts_preferences(&mut self, r: &Req<'_>) -> Value {
        let version = if r.s("GlobalEndpointTokenVersion") == "v2Token" {
            2
        } else {
            1
        };
        self.held
            .prefs
            .insert(TOKEN_VERSION.to_owned(), Pref::TokenVersion(version));
        Value::Null
    }

    fn token_version(&self) -> u8 {
        match self.held.prefs.get(TOKEN_VERSION) {
            Some(Pref::TokenVersion(v)) => *v,
            Some(Pref::Created(_)) | None => 1,
        }
    }

    pub(crate) fn update_password_policy(&mut self, r: &Req<'_>) -> Result<Value, ApiError> {
        let mut m = Map::new();
        for (k, default) in POLICY_DEFAULTS {
            // MaxPasswordAge and PasswordReusePrevention are left out when they are not set (zero), as AWS leaves them out.
            let n = r.int(k).unwrap_or(default);
            if n > 0 {
                m.insert(k.to_owned(), json!(n));
            }
        }
        for k in POLICY_FLAGS {
            m.insert(k.to_owned(), json!(r.flag(k).unwrap_or(false)));
        }
        m.insert(
            "ExpirePasswords".to_owned(),
            json!(r.int("MaxPasswordAge").unwrap_or(0) > 0),
        );
        let text = Value::Object(m).to_string();
        if let Some(a) = self.account_mut() {
            a.password_policy = Some(text);
        }
        Ok(Value::Null)
    }

    pub(crate) fn get_password_policy(&self, r: &Req<'_>) -> Result<Value, ApiError> {
        let text = self
            .account()
            .password_policy
            .ok_or_else(|| no_policy(r.account()))?;
        Ok(json!({"PasswordPolicy": serde_json::from_str::<Value>(&text).unwrap_or(Value::Null)}))
    }

    pub(crate) fn delete_password_policy(&mut self, r: &Req<'_>) -> Result<Value, ApiError> {
        let had = self.account().password_policy.is_some();
        if !had {
            return Err(no_policy(r.account()));
        }
        if let Some(a) = self.account_mut() {
            a.password_policy = None;
        }
        Ok(Value::Null)
    }

    pub(crate) fn create_login_profile(&mut self, r: &Req<'_>) -> Result<Value, ApiError> {
        let now = self.now_ms();
        let name = r.s("UserName");
        if self.entity(Kind::User, name)?.password_set_ms.is_some() {
            return Err(exists("Login Profile", name));
        }
        let reset = r.flag("PasswordResetRequired").unwrap_or(false);
        if let Some(password) = r.opt("Password") {
            self.set_password(name, password, reset)?;
        }
        let u = self.entity_mut(Kind::User, name)?;
        u.password_set_ms = Some(now);
        Ok(
            json!({"LoginProfile": {"UserName": u.name, "CreateDate": secs(now), "PasswordResetRequired": reset}}),
        )
    }

    pub(crate) fn get_login_profile(&self, r: &Req<'_>) -> Result<Value, ApiError> {
        let u = self.entity(Kind::User, r.s("UserName"))?;
        let at = u.password_set_ms.ok_or_else(|| no_profile(&u.name))?;
        let reset = self
            .held
            .passwords
            .get(&u.name)
            .is_some_and(|p| p.reset_required);
        Ok(
            json!({"LoginProfile": {"UserName": u.name, "CreateDate": secs(at), "PasswordResetRequired": reset}}),
        )
    }

    pub(crate) fn delete_login_profile(&mut self, r: &Req<'_>) -> Result<Value, ApiError> {
        let u = self.entity_mut(Kind::User, r.s("UserName"))?;
        if u.password_set_ms.take().is_none() {
            return Err(no_profile(&u.name));
        }
        self.held.passwords.remove(r.s("UserName"));
        Ok(Value::Null)
    }

    pub(crate) fn update_login_profile(&mut self, r: &Req<'_>) -> Result<Value, ApiError> {
        let name = r.s("UserName");
        let u = self.entity(Kind::User, name)?;
        if u.password_set_ms.is_none() {
            return Err(no_profile(&u.name));
        }
        let reset = r.flag("PasswordResetRequired");
        if let Some(password) = r.opt("Password") {
            self.set_password(name, password, reset.unwrap_or(false))?;
        } else if let Some(reset) = reset
            && let Some(p) = self.held.passwords.get_mut(name)
        {
            p.reset_required = reset;
        }
        Ok(Value::Null)
    }

    /// `CreateServiceLinkedRole`: a role named `AWSServiceRoleFor<Service>` under `/aws-service-role/<service>/`, trusting the service.
    pub(crate) fn create_service_linked_role(&mut self, r: &Req<'_>) -> Result<Value, ApiError> {
        let service = r.s("AWSServiceName");
        let stem: String = service
            .split('.')
            .next()
            .unwrap_or(service)
            .split('-')
            .map(capitalise)
            .collect();
        let name = format!(
            "AWSServiceRoleFor{stem}{}",
            r.opt("CustomSuffix")
                .map(|s| format!("_{s}"))
                .unwrap_or_default()
        );
        if self
            .entities
            .get(&crate::core::entity_key(Kind::Role, &name))
            .is_some()
        {
            // (memory)
            return Err(ApiError::modeled(
                "InvalidInputException",
                format!(
                    "Service role name {name} has been taken in this account, please try a different suffix."
                ),
            ));
        }
        let path = format!("/aws-service-role/{service}/");
        let trust = json!({"Version": "2012-10-17", "Statement": [{"Effect": "Allow", "Principal": {"Service": service}, "Action": "sts:AssumeRole"}]}).to_string();
        let e = Entity {
            kind: Kind::Role,
            id: new_id("AROA")?,
            arn: arn_of(r.account(), "role", &path, &name),
            name: name.clone(),
            path,
            created_ms: self.now_ms(),
            tags: BTreeMap::new(),
            inline: BTreeMap::new(),
            attached: BTreeSet::new(),
            boundary: None,
            trust: Some(trust),
            description: r.opt("Description").map(str::to_owned),
            max_session_secs: 3600,
            last_used: None,
            service_linked: Some(service.to_owned()),
            groups: BTreeSet::new(),
            password_set_ms: None,
        };
        let shown = crate::view::entity(&e, false);
        self.entities
            .insert(crate::core::entity_key(Kind::Role, &name), e);
        Ok(json!({"Role": shown}))
    }

    pub(crate) fn delete_service_linked_role(&mut self, r: &Req<'_>) -> Result<Value, ApiError> {
        let name = r.s("RoleName");
        let e = self.entity(Kind::Role, name)?;
        if e.service_linked.is_none() {
            return Err(ApiError::modeled(
                "InvalidInputException",
                format!("Role {name} is not a service-linked role."),
            ));
        }
        if !e.attached.is_empty() || !e.inline.is_empty() {
            return Err(conflict(
                "Cannot delete entity, must detach all policies first.",
            ));
        }
        self.entities
            .remove(&crate::core::entity_key(Kind::Role, name));
        // The deletion is finished at once (SCOPE.md: asynchronous deletion is not built).
        Ok(json!({"DeletionTaskId": format!("task{}/{name}/{}", self.now_ms(), new_id("")?)}))
    }

    pub(crate) fn service_linked_role_deletion_status(&self) -> Value {
        json!({"Status": "SUCCEEDED"})
    }
}

fn no_policy(account: &str) -> ApiError {
    ApiError::modeled(
        "NoSuchEntityException",
        format!("The Password Policy with domain name {account} cannot be found."),
    )
}

pub(crate) fn no_profile(user: &str) -> ApiError {
    // (memory)
    ApiError::modeled(
        "NoSuchEntityException",
        format!("Login Profile for User {user} cannot be found."),
    )
}

fn capitalise(word: &str) -> String {
    let mut c = word.chars();
    c.next()
        .map(|f| f.to_uppercase().collect::<String>() + c.as_str())
        .unwrap_or_default()
}
