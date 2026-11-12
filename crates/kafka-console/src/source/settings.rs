//! What the Settings and Access pages ask of the source: the live broker (DescribeConfigs, IncrementalAlterConfigs,
//! DescribeAcls, CreateAcls, DeleteAcls, DescribeUserScramCredentials through `Broker::handle` as the console's principal,
//! and `ConsoleView`) or, in the tests and the `boards` example, the larder.

use super::Source;
use crate::gather::access_dialogs::AclFields;
use crate::gather::settings_edit::SettingsFields;
use crate::gather::{self};
use crate::model::{
    AccessView, AclCreateResult, AclCreateView, AclDeleteResult, AclDeleteView, AclEntry,
    SettingsEditView, SettingsResult, SettingsView,
};

impl Source {
    /// The Settings page. `hint` names a state for the fixtures only.
    pub async fn settings(&self) -> SettingsView {
        match self {
            Self::Live(l) => {
                let now = super::now_ms();
                // The tape names the clients; a console with no recorder names none.
                let heard = l
                    .recorder
                    .as_ref()
                    .map_or_else(Vec::new, |r| r.heard(&gather::settings_live::SPOKEN));
                gather::settings::settings(&l.backend, (&l.facts, &l.start), &heard, now).await
            }
            #[cfg(any(test, feature = "examples"))]
            Self::Fixture => crate::fixtures_settings::page(),
        }
    }

    /// The edit dialog of a dynamic broker config; `None` if it is not one of the five.
    pub async fn settings_form(&self, key: &str, scope: &str) -> Option<SettingsEditView> {
        match self {
            Self::Live(l) => gather::settings_edit::form(&l.backend, key, scope).await,
            #[cfg(any(test, feature = "examples"))]
            Self::Fixture => crate::fixtures_settings::form(key, scope),
        }
    }

    /// Saves the form, or with `check` asks the broker whether it would take it.
    pub async fn settings_save(&self, f: SettingsFields<'_>, check: bool) -> SettingsResult {
        match self {
            Self::Live(l) => gather::settings_edit::save(&l.backend, f, check).await,
            #[cfg(any(test, feature = "examples"))]
            Self::Fixture => crate::fixtures_settings::save(f, check),
        }
    }

    /// The Access page. `hint` names a state for the fixtures only (`secured`).
    pub async fn access(&self, hint: Option<&str>) -> AccessView {
        match self {
            Self::Live(l) => {
                let _ = hint;
                gather::access::access(&l.backend, &l.facts, &l.start).await
            }
            #[cfg(any(test, feature = "examples"))]
            Self::Fixture => crate::fixtures_access::page(hint == Some("secured")),
        }
    }

    /// The Create ACL dialog for what was typed.
    pub async fn acl_create_form(&self, f: &AclFields<'_>) -> AclCreateView {
        match self {
            Self::Live(l) => {
                let version = l.backend.version(kafka_wire::messages::ApiKey::CreateAcls);
                gather::access_dialogs::create_view(
                    version,
                    gather::access_dialogs::input_of(f),
                    None,
                )
            }
            #[cfg(any(test, feature = "examples"))]
            Self::Fixture => crate::fixtures_access::create_form(f),
        }
    }

    /// Makes the ACL.
    pub async fn acl_create(&self, f: &AclFields<'_>) -> AclCreateResult {
        match self {
            Self::Live(l) => gather::access_dialogs::create(&l.backend, f).await,
            #[cfg(any(test, feature = "examples"))]
            Self::Fixture => crate::fixtures_access::create(f),
        }
    }

    /// The Delete ACL dialog for one ACL.
    pub async fn acl_delete_form(&self, target: AclEntry) -> AclDeleteView {
        match self {
            Self::Live(l) => gather::access_dialogs::delete_form(&l.backend, target).await,
            #[cfg(any(test, feature = "examples"))]
            Self::Fixture => crate::fixtures_access::delete_form(target),
        }
    }

    /// Deletes the one ACL.
    pub async fn acl_delete(&self, target: AclEntry) -> AclDeleteResult {
        match self {
            Self::Live(l) => gather::access_dialogs::delete(&l.backend, target).await,
            #[cfg(any(test, feature = "examples"))]
            Self::Fixture => crate::fixtures_access::delete(target),
        }
    }
}
