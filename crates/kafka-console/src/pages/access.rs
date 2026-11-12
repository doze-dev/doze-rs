//! The Access page (slice 9h), written from the boards `access-open` and `access-secured` and their two dialogs
//! `access-acl-create` and `access-acl-delete`: what is on and off (sign-in, ACLs, TLS), the users by name, the ACLs with
//! Create and Delete, the console's own principal and what protects the console, and the three start-up warnings.
//!
//! One page serves both boards: the data decides which one it is. What the console does differently from the boards, and why:
//! - The users table has no Iterations column: the iteration count of a credential is not shown anywhere (it is read by no
//!   code of the console), so no part of a credential can reach a page. The mechanism is shown.
//! - Create ACL is offered on an open broker too (an ACL binds `User:ANONYMOUS` there, and the broker accepts it), and the
//!   board's "See the page with security on" button, which is a canvas device, is not drawn.
//! - The console card says in words that anyone who can reach the console acts as `User:__console` (there is no login).

mod acls;
mod cards;
mod console;
mod dialogs;
mod page;
mod users;

pub use dialogs::{acl_create_dialog, acl_create_title, acl_delete_dialog, acl_delete_title};
pub use page::access_page;

use crate::base::Base;

/// The address of the Create ACL dialog.
#[must_use]
pub fn create_url(base: &Base) -> String {
    base.url("/access/acls/new")
}

/// The address of the Delete ACL dialog of one ACL.
#[must_use]
pub fn delete_url(base: &Base, query: &str) -> String {
    base.url(&format!("/access/acls/delete?{query}"))
}

/// The address of the page.
#[must_use]
pub fn access_url(base: &Base) -> String {
    base.url("/access")
}

#[cfg(test)]
mod tests;
