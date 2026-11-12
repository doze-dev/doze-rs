#![no_main]
//! Any ACL, any filter: never a panic, and the rules that hold whatever the bytes were. The codes are raw `i8`s
//! (a client may send any), the strings are anything.

use kafka_auth::authz::{
    Acl, AclFilter, AclSet, Resource, Rules, Subject, operation, pattern, permission, resource,
};
use libfuzzer_sys::arbitrary::{Result, Unstructured};
use libfuzzer_sys::fuzz_target;

fn text(u: &mut Unstructured<'_>) -> Result<String> {
    // Short strings from a small alphabet collide often, which is where the matching rules are.
    let len = u.int_in_range(0..=4)?;
    (0..len)
        .map(|_| Ok(*u.choose(&['a', 'b', '*', ':', '\0'])?))
        .collect()
}

fn maybe(u: &mut Unstructured<'_>) -> Result<Option<String>> {
    if u.arbitrary::<bool>()? {
        text(u).map(Some)
    } else {
        Ok(None)
    }
}

fn acl(u: &mut Unstructured<'_>) -> Result<Acl> {
    Ok(Acl {
        resource_type: u.arbitrary()?,
        name: text(u)?,
        pattern_type: u.arbitrary()?,
        principal: text(u)?,
        host: text(u)?,
        operation: u.arbitrary()?,
        permission: u.arbitrary()?,
    })
}

fn filter(u: &mut Unstructured<'_>) -> Result<AclFilter> {
    Ok(AclFilter {
        resource_type: u.arbitrary()?,
        name: maybe(u)?,
        pattern_type: u.arbitrary()?,
        principal: maybe(u)?,
        host: maybe(u)?,
        operation: u.arbitrary()?,
        permission: u.arbitrary()?,
    })
}

fn check(u: &mut Unstructured<'_>) -> Result<()> {
    let acls = [acl(u)?, acl(u)?, acl(u)?];
    let (f, g) = (filter(u)?, filter(u)?);

    // Zero is found exactly where it is.
    let zero = |codes: [i8; 4]| codes.contains(&0);
    assert_eq!(
        f.has_zero_code(),
        zero([f.resource_type, f.pattern_type, f.operation, f.permission])
    );

    // A filter that validates has four codes that are values of their enumerations.
    if f.validate().is_ok() {
        assert!(resource::is_known(f.resource_type) && pattern::is_known(f.pattern_type));
        assert!(operation::is_known(f.operation) && permission::is_known(f.permission));
    }

    for a in &acls {
        // The filter of everything matches everything; no filter matches an ACL it was not built from by luck
        // only: matching is a pure function of the two.
        assert!(AclFilter::any().matches(a));
        assert_eq!(f.matches(a), f.matches(&a.clone()));

        // An ACL that can be stored has codes a filter would accept, a name and a principal with a colon.
        if a.refusal().is_none() {
            assert!(!a.name.is_empty() && a.principal.contains(':'));
            assert!(!a.has_zero_code() && !a.has_filter_only_code());
        }

        // The ACL's own fields, as a filter, select it.
        let own = AclFilter {
            resource_type: a.resource_type,
            name: Some(a.name.clone()),
            pattern_type: a.pattern_type,
            principal: Some(a.principal.clone()),
            host: Some(a.host.clone()),
            operation: a.operation,
            permission: a.permission,
        };
        assert!(own.matches(a));
    }

    // The set selects what the filter matches, each ACL once.
    let set = AclSet::new(acls.iter().cloned());
    let manual = set.iter().filter(|a| g.matches(a)).count();
    assert_eq!(set.select(&g).len(), manual);

    // The decision does not panic on whatever was stored, for any caller.
    let (who, host) = (text(u)?, text(u)?);
    let rules = Rules {
        default_allow: u.arbitrary()?,
        super_users: std::iter::once(text(u)?).collect(),
    };
    let subject = Subject { principal: &who, host: &host };
    let (kind, op): (i8, i8) = (u.arbitrary()?, u.arbitrary()?);
    let name = text(u)?;
    let _ = rules.authorize(&set, subject, Resource { kind, name: &name }, op);
    let _ = rules.authorize_by_resource_type(&set, subject, op, kind);
    Ok(())
}

fuzz_target!(|data: &[u8]| {
    let _ = check(&mut Unstructured::new(data));
});
