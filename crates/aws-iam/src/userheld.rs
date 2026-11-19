//! What a user holds beside its access keys and groups: a console password, MFA devices, signing certificates, SSH keys and
//! service-specific credentials. A user is not deleted while it holds any (the deploy tools delete each first), and a renamed user
//! keeps them all.

use crate::core::Core;
use crate::held::CredKind;
use crate::model::Kind;

impl Core {
    /// What must go before `user` can be deleted, in the words of AWS's refusal (memory: AWS's order and words, after the groups and
    /// keys, which were recorded).
    pub(crate) fn user_still_has(&self, user: &str) -> Option<&'static str> {
        if self
            .entity(Kind::User, user)
            .is_ok_and(|u| u.password_set_ms.is_some())
        {
            return Some("delete login profile");
        }
        if self.devices_of(user).next().is_some() {
            return Some("delete MFA device");
        }
        let held = |want: fn(&CredKind) -> bool| {
            self.held
                .creds
                .iter()
                .any(|(_, c)| c.user == user && want(&c.kind))
        };
        if held(|k| matches!(k, CredKind::Signing { .. })) {
            return Some("delete signing certificates");
        }
        if held(|k| matches!(k, CredKind::Ssh { .. })) {
            return Some("delete SSH public keys");
        }
        if held(|k| matches!(k, CredKind::Service(_))) {
            return Some("delete service specific credentials");
        }
        None
    }

    /// A user renamed: its password, devices and credentials follow it.
    pub(crate) fn held_renamed(&mut self, old: &str, new: &str) {
        if let Some(p) = self.held.passwords.remove(old) {
            self.held.passwords.insert(new.to_owned(), p);
        }
        let serials: Vec<String> = self.devices_of(old).map(|d| d.serial.clone()).collect();
        for serial in serials {
            if let Some(d) = self.held.mfa.get_mut(&serial)
                && let Some((user, _)) = d.user.as_mut()
            {
                new.clone_into(user);
            }
        }
        let ids: Vec<String> = self
            .held
            .creds
            .iter()
            .filter(|(_, c)| c.user == old)
            .map(|(id, _)| id.to_owned())
            .collect();
        for id in ids {
            if let Some(c) = self.held.creds.get_mut(&id) {
                new.clone_into(&mut c.user);
            }
        }
    }
}
