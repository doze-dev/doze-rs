//! The `postgres` block: one type gives the schema the core checks, the registry's documentation and the decoder, so
//! they cannot disagree (Go's description called `grant` labelled, "role", while its decoder took no label).

use doze_module::Spec;
use std::collections::BTreeMap;

#[derive(Debug, Spec)]
pub(crate) struct PostgresSpec {
    #[spec(doc = "Owner role for the instance's database.")]
    pub owner: Option<String>,
    #[spec(doc = "Character-set encoding for the database; the cluster's (UTF8) when left out.")]
    pub encoding: Option<String>,
    #[spec(doc = "Locale (LC_COLLATE and LC_CTYPE) for the database.")]
    pub locale: Option<String>,
    #[spec(doc = "Collation order, overriding locale.")]
    pub lc_collate: Option<String>,
    #[spec(doc = "Character classification, overriding locale.")]
    pub lc_ctype: Option<String>,
    #[spec(doc = "Template database to create from.")]
    pub template: Option<String>,
    #[spec(doc = "Tablespace the database lives in.")]
    pub tablespace: Option<String>,
    #[spec(doc = "COMMENT on the database; removing it clears it.")]
    pub comment: Option<String>,
    #[spec(doc = "Max concurrent connections to the database; no limit (-1) when left out.")]
    pub connection_limit: Option<i64>,
    #[spec(default = false, doc = "Mark the database as a template.")]
    pub is_template: bool,
    #[spec(default = true, doc = "Whether the database accepts connections.")]
    pub allow_connections: bool,
    #[spec(
        default = "16MB",
        doc = "shared_buffers server setting, e.g. \"256MB\"."
    )]
    pub shared_buffers: String,
    #[spec(default = 50, doc = "max_connections server setting.")]
    pub max_connections: i64,
    #[spec(
        default = false,
        doc = "fsync server setting (off for the dev tuning profile; synchronous_commit and full_page_writes follow it)."
    )]
    pub fsync: bool,
    #[spec(default = true, doc = "autovacuum server setting.")]
    pub autovacuum: bool,
    #[spec(doc = "Any other postgresql.conf setting, applied verbatim.")]
    pub settings: BTreeMap<String, String>,
    #[spec(
        doc = "Extensions to CREATE, by name (contrib: pg_trgm, hstore, …). Or use extension blocks."
    )]
    pub extensions: Vec<String>,
    #[spec(block, doc = "A role (a user when it may log in), kept as declared.")]
    pub role: Vec<Role>,
    #[spec(block, doc = "A schema within the database.")]
    pub schema: Vec<Schema>,
    #[spec(block, doc = "A PostgreSQL extension created in the database.")]
    pub extension: Vec<Extension>,
    #[spec(
        block,
        doc = "Privileges for a role on the database, or on a schema or its objects."
    )]
    pub grant: Vec<Grant>,
}

#[derive(Debug, Spec)]
pub(crate) struct Role {
    #[spec(label)]
    pub name: String,
    #[spec(doc = "Login password; removing it clears it.")]
    pub password: Option<String>,
    #[spec(default = true, doc = "Whether the role may log in.")]
    pub login: bool,
    #[spec(default = false, doc = "Grant SUPERUSER.")]
    pub superuser: bool,
    #[spec(default = false, doc = "Allow creating databases.")]
    pub createdb: bool,
    #[spec(default = false, doc = "Allow creating other roles.")]
    pub createrole: bool,
    #[spec(default = false, doc = "Allow streaming replication.")]
    pub replication: bool,
    #[spec(
        default = true,
        doc = "Inherit the privileges of the roles it is a member of."
    )]
    pub inherit: bool,
    #[spec(default = false, doc = "Bypass row-level security.")]
    pub bypassrls: bool,
    #[spec(doc = "Per-role connection cap; none (-1) when left out.")]
    pub connection_limit: Option<i64>,
    #[spec(doc = "Password expiry timestamp; removing it means never.")]
    pub valid_until: Option<String>,
    #[spec(doc = "Roles this role is a member of; one removed is revoked.")]
    pub member_of: Vec<String>,
    #[spec(doc = "COMMENT on the role; removing it clears it.")]
    pub comment: Option<String>,
    #[spec(doc = "Per-role settings (ALTER ROLE … SET); one removed is reset.")]
    pub config: BTreeMap<String, String>,
}

#[derive(Debug, Spec)]
pub(crate) struct Schema {
    #[spec(label)]
    pub name: String,
    #[spec(doc = "Role that owns the schema.")]
    pub owner: Option<String>,
}

#[derive(Debug, Spec)]
pub(crate) struct Extension {
    #[spec(label)]
    pub name: String,
    #[spec(doc = "The SQL version to create; a different one later updates it in place.")]
    pub version: Option<String>,
    #[spec(doc = "Schema to install the extension into.")]
    pub schema: Option<String>,
    #[spec(
        doc = "Path of a bundle (.tar.gz with share/ and lib/) for an extension the toolchain does not ship, relative to this file."
    )]
    pub source: Option<String>,
    #[spec(
        default = false,
        doc = "Skip (don't fail) if the extension is unavailable."
    )]
    pub optional: bool,
    #[spec(
        default = false,
        doc = "CREATE EXTENSION … CASCADE for its dependencies."
    )]
    pub cascade: bool,
}

#[derive(Debug, Spec)]
pub(crate) struct Grant {
    #[spec(doc = "The role granted to.")]
    pub role: String,
    #[spec(doc = "Privileges to grant (SELECT, INSERT, ALL, …).")]
    pub privileges: Vec<String>,
    #[spec(doc = "The database granted on (one of database or schema).")]
    pub database: Option<String>,
    #[spec(doc = "The schema granted on.")]
    pub schema: Option<String>,
    #[spec(
        doc = "Objects in the schema: tables, sequences or functions; the schema itself when left out."
    )]
    pub objects: Option<String>,
    #[spec(default = false, doc = "Allow the grantee to re-grant.")]
    pub with_grant_option: bool,
}

/// One extension as converge sees it: the list shorthand and the blocks together, the list's names first (Go's order).
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Ext {
    pub name: String,
    pub version: Option<String>,
    pub schema: Option<String>,
    pub source: Option<String>,
    pub optional: bool,
    pub cascade: bool,
}

impl PostgresSpec {
    /// Every declared extension once: the list's names (a repeat there is skipped, as Go did), then the blocks.
    pub(crate) fn exts(&self) -> Vec<Ext> {
        let mut out: Vec<Ext> = Vec::new();
        for n in &self.extensions {
            let n = crate::ext::canonical(n);
            if !out.iter().any(|e| e.name == n) {
                out.push(Ext {
                    name: n,
                    version: None,
                    schema: None,
                    source: None,
                    optional: false,
                    cascade: false,
                });
            }
        }
        for e in &self.extension {
            let name = crate::ext::canonical(&e.name);
            out.retain(|o| o.name != name);
            out.push(Ext {
                name,
                version: e.version.clone().filter(|v| !v.is_empty()),
                schema: e.schema.clone().filter(|v| !v.is_empty()),
                source: e.source.clone().filter(|v| !v.is_empty()),
                optional: e.optional,
                cascade: e.cascade,
            });
        }
        out
    }

    /// The database's collation and character classification: each falls back to `locale`.
    pub(crate) fn collate_ctype(&self) -> (Option<&str>, Option<&str>) {
        let locale = some(&self.locale);
        (
            some(&self.lc_collate).or(locale),
            some(&self.lc_ctype).or(locale),
        )
    }
}

/// A string argument that is set and not empty (Go read "" as unset everywhere).
pub(crate) fn some(v: &Option<String>) -> Option<&str> {
    v.as_deref().filter(|s| !s.is_empty())
}
