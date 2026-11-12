use super::*;
use crate::fixtures_access as larder;
use crate::gather::access_dialogs::AclFields;

fn open() -> String {
    access_page(&larder::page(false), &Base::root()).into_string()
}

fn secured() -> String {
    access_page(&larder::page(true), &Base::root()).into_string()
}

#[test]
fn the_open_broker_says_what_is_off_and_the_secured_one_what_is_on() {
    let o = open();
    assert!(
        o.contains("<span class=\"st idle\">Open</span>"),
        "{o:.1500}"
    );
    assert!(
        o.contains("Allows by default") && o.contains("No ACLs") && o.contains("User:ANONYMOUS")
    );
    assert!(
        !o.contains("<h2>Users</h2>"),
        "an open broker has no sign-in, so no users table"
    );
    let s = secured();
    assert!(
        s.contains("<span class=\"st ok\">Sign-in required</span>"),
        "{s:.1500}"
    );
    assert!(
        s.contains("SCRAM-SHA-256, SCRAM-SHA-512")
            && s.contains("Denies by default")
            && s.contains("TLS on")
    );
    assert!(
        s.contains("1 from flags, 3 stored")
            && s.contains("7 allow, 1 deny")
            && s.contains("User:admin")
    );
    assert!(
        s.contains("~/larder/tls/broker.pem") && s.contains("its contents are printed nowhere")
    );
}

#[test]
fn every_acl_has_a_delete_that_names_it_exactly() {
    let s = secured();
    assert_eq!(s.matches("data-op=\"DeleteAcls\"").count(), 8);
    assert!(
        s.contains("href=\"/access/acls/delete?type=topic&amp;name=orders&amp;pattern=literal&amp;principal=User%3Aorders&amp;host=%2A&amp;op=write&amp;perm=allow\""),
        "{s}"
    );
    assert!(s.contains("<span class=\"st warn\">Deny</span>"));
}

#[test]
fn the_console_card_says_who_it_acts_as_and_that_anyone_who_can_reach_it_does() {
    let o = open();
    assert!(
        o.contains("User:__console") && o.contains("Super user"),
        "{o}"
    );
    assert!(
        o.contains("Anyone who can reach it") && o.contains("there is no login"),
        "{o}"
    );
    assert!(o.contains("Same origin only"));
    let mut v = larder::page(false);
    v.console.beyond_loopback = true;
    v.console.listens = "0.0.0.0:9093".into();
    let p = access_page(&v, &Base::root()).into_string();
    assert!(
        p.contains("The console is served beyond loopback with no login."),
        "{p}"
    );
}

#[test]
fn a_warning_that_holds_is_marked_and_the_rest_are_not_the_case() {
    let o = open();
    assert_eq!(o.matches("not the case").count(), 3);
    let mut v = larder::page(true);
    v.warnings[2].holds = true;
    let p = access_page(&v, &Base::root()).into_string();
    assert_eq!(p.matches("not the case").count(), 2);
    assert!(p.contains(">applies<"));
}

#[test]
fn no_secret_field_exists_to_be_shown() {
    let s = secured();
    // The users table has the board's four columns; the iteration count is a public parameter, and no secret part is a field.
    assert!(
        s.contains("<th scope=\"col\">User</th><th scope=\"col\">From</th><th scope=\"col\">Mechanisms</th><th scope=\"col\">Iterations</th>"),
        "{s}"
    );
    assert!(s.contains("<td class=\"mono\">4,096</td>"), "{s}");
    for word in ["stored_key", "server_key", "Salt", "salted"] {
        assert!(!s.contains(word), "{word} on the page");
    }
}

#[test]
fn hostile_names_are_escaped_in_the_table_and_in_the_delete_address() {
    let mut v = larder::page(true);
    v.acls.rows[0].name = "<script>alert(1)</script>".into();
    v.acls.rows[0].principal = "User:\"><img src=x>".into();
    v.users[1].name = "<b>x</b>".into();
    let p = access_page(&v, &Base::root()).into_string();
    assert!(
        !p.contains("<script>alert") && !p.contains("<img src=x") && !p.contains("<b>x</b>"),
        "{p}"
    );
    assert!(
        p.contains("name=%3Cscript%3Ealert%281%29%3C%2Fscript%3E"),
        "the address is encoded"
    );
}

#[test]
fn the_dialogs_are_forms_that_post_to_their_own_address_and_name_their_call() {
    let f = AclFields {
        kind: "topic",
        name: "orders",
        pattern: "literal",
        principal: "User:orders",
        host: "*",
        op: "write",
        perm: "allow",
    };
    let c = acl_create_dialog(&larder::create_form(&f), &Base::root()).into_string();
    assert!(
        c.contains("action=\"/access/acls/new\"") && c.contains("data-op=\"CreateAcls\""),
        "{c}"
    );
    assert!(
        c.contains("The call, asked as User:__console")
            && c.contains("resource_name: <span class=\"s\">orders</span>"),
        "{c}"
    );
    assert!(
        c.contains("<option value=\"topic\" selected>")
            && c.contains("<option value=\"write\" selected>")
    );
    assert!(c.contains("name=\"perm\" value=\"allow\" checked"));
    let d = acl_delete_dialog(
        &larder::delete_form(larder::acls().remove(0)),
        &Base::root(),
    )
    .into_string();
    assert!(
        d.contains("action=\"/access/acls/delete\"") && d.contains("class=\"btn danger fill\""),
        "{d}"
    );
    assert!(
        d.contains("It takes effect on the next request.")
            && d.contains("TOPIC_AUTHORIZATION_FAILED"),
        "{d}"
    );
    let gone = acl_delete_dialog(
        &larder::delete_form(larder::acls().remove(1).clone_with("nothing")),
        &Base::root(),
    )
    .into_string();
    assert!(
        gone.contains("This ACL is not there any more") && !gone.contains("Delete ACL</button>"),
        "{gone}"
    );
}

trait CloneWith {
    fn clone_with(self, name: &str) -> Self;
}

impl CloneWith for crate::model::AclEntry {
    fn clone_with(mut self, name: &str) -> Self {
        self.name = name.to_owned();
        self
    }
}
