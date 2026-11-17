use mesh_store::ha::AuthorityScope;

#[test]
fn canonical_scope_round_trips_without_aliases() {
    let scope = AuthorityScope::new("install-a", "tenant-a", "workspace-a", "dev").unwrap();
    assert_eq!(scope.workspace_scope(), "tenant-a/workspace-a/dev");
    assert_eq!(
        scope.storage_key(),
        "mesh-authority-v1/install-a/tenant-a/workspace-a/dev"
    );
    assert_eq!(
        AuthorityScope::from_storage_key(&scope.storage_key()).unwrap(),
        scope
    );
    assert_eq!(
        serde_json::from_value::<AuthorityScope>(serde_json::to_value(&scope).unwrap()).unwrap(),
        scope
    );
    for component in [
        "", ".", "..", "a/b", "a:b", "a%2fb", " a", "a ", "a\n", "a\\b", "é",
    ] {
        assert!(
            AuthorityScope::new(component, "t", "w", "e").is_err(),
            "{component:?}"
        );
        assert!(AuthorityScope::new("i", component, "w", "e").is_err());
        assert!(AuthorityScope::new("i", "t", component, "e").is_err());
        assert!(AuthorityScope::new("i", "t", "w", component).is_err());
    }
    for key in [
        "i/t/w/e",
        "mesh-authority-v1/i/t/w/e/",
        "mesh-authority-v1/i/t/w",
        "mesh-authority-v2/i/t/w/e",
    ] {
        assert!(AuthorityScope::from_storage_key(key).is_err());
    }
    assert!(
        serde_json::from_value::<AuthorityScope>(serde_json::json!({
            "installation_id":"i", "tenant_id":"t/w", "workspace_id":"w", "environment_id":"e"
        }))
        .is_err()
    );
}

#[test]
fn every_authority_dimension_is_part_of_the_key() {
    let scopes = [
        ("i", "t", "w", "e"),
        ("j", "t", "w", "e"),
        ("i", "u", "w", "e"),
        ("i", "t", "x", "e"),
        ("i", "t", "w", "f"),
    ];
    let keys: std::collections::BTreeSet<_> = scopes
        .into_iter()
        .map(|(i, t, w, e)| AuthorityScope::new(i, t, w, e).unwrap().storage_key())
        .collect();
    assert_eq!(keys.len(), scopes.len());
}
