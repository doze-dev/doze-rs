//! The editor used as `doze add` uses it on a new file: the stack set first, then a block. hclwrite's body has the
//! attribute as an item once it is set, so the block comes after a blank line (Go's oracle for `doze add`, recorded
//! in doze-cli's tests/fixtures/go/add-init.json, found the missing blank line).
#![allow(clippy::unwrap_used)]

use doze_hcl::edit::Edit;

#[test]
fn a_block_after_the_stack_of_a_new_file_follows_a_blank_line() {
    let mut e = Edit::open("").unwrap();
    e.set_top_attribute("stack", "shop");
    e.append_block("module valkey \"cache\" {\n  version = 9\n}")
        .unwrap();
    assert_eq!(
        String::from_utf8(e.bytes()).unwrap(),
        "stack = \"shop\"\n\nmodule valkey \"cache\" {\n  version = 9\n}\n"
    );
}
