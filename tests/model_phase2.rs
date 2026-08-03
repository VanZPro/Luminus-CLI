#[path = "../src/model.rs"]
mod model;

use model::{ModelCatalog, ModelCatalogError, ModelRole, ModelSelection};

#[test]
fn roles_are_typed_and_stable() {
    assert_eq!(ModelRole::ALL.len(), 8);
    assert_eq!("DEEP".parse::<ModelRole>().unwrap(), ModelRole::Deep);
    assert_eq!(ModelRole::Vision.to_string(), "vision");
    assert!("unknown".parse::<ModelRole>().is_err());
}

#[test]
fn catalog_selects_and_lists_deterministically() {
    let mut catalog = ModelCatalog::new();
    catalog
        .add(ModelSelection::new("acme", "slow", ModelRole::Deep))
        .unwrap();
    catalog
        .add(ModelSelection::new("local", "quick", ModelRole::Fast))
        .unwrap();
    assert_eq!(catalog.list()[0].model, "slow");
    assert!(catalog.active().is_none());
    assert_eq!(catalog.select_role("fast").unwrap().model, "quick");
    assert_eq!(catalog.active().unwrap().provider, "local");
}

#[test]
fn adding_same_role_replaces_without_reordering() {
    let mut catalog = ModelCatalog::new();
    catalog
        .add(ModelSelection::new("a", "one", ModelRole::Default))
        .unwrap();
    catalog
        .add(ModelSelection::new("b", "two", ModelRole::Fast))
        .unwrap();
    catalog
        .add(ModelSelection::new("c", "three", ModelRole::Default))
        .unwrap();
    assert_eq!(catalog.list().len(), 2);
    assert_eq!(catalog.list()[0].model, "three");
    assert_eq!(catalog.list()[1].model, "two");
}

#[test]
fn validates_unknown_roles_models_and_empty_values() {
    let mut catalog = ModelCatalog::new();
    assert!(matches!(
        catalog.select_role("missing"),
        Err(ModelCatalogError::UnknownRole(_))
    ));
    assert!(matches!(
        catalog.validate_model("missing"),
        Err(ModelCatalogError::UnknownModel(_))
    ));
    assert!(matches!(
        catalog.add(ModelSelection::new("", "x", ModelRole::Fast)),
        Err(ModelCatalogError::EmptyProvider)
    ));
    assert!(matches!(
        catalog.add(ModelSelection::new("p", "", ModelRole::Fast)),
        Err(ModelCatalogError::EmptyModel)
    ));
}
