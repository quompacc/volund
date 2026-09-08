use std::collections::HashSet;
use std::path::Path;

use serde_json::Value;

const MAX_MANIFEST_BYTES: usize = 16 * 1024 * 1024;
const MAX_NODES: usize = 20_000;
const MAX_DEPTH: usize = 256;

pub async fn inspect(derived_root: &Path, relative_path: &str, byte_size: i64) -> Option<String> {
    let Ok(recorded_size) = usize::try_from(byte_size) else {
        return Some("Das STEP-Strukturmanifest überschreitet die zulässige Größe.".to_owned());
    };
    if recorded_size > MAX_MANIFEST_BYTES {
        return Some("Das STEP-Strukturmanifest überschreitet die zulässige Größe.".to_owned());
    }
    let root = tokio::fs::canonicalize(derived_root).await.ok()?;
    let artifact = tokio::fs::canonicalize(root.join(relative_path))
        .await
        .ok()?;
    if !artifact.starts_with(&root) {
        return None;
    }
    let bytes = tokio::fs::read(artifact).await.ok()?;
    if bytes.len() > MAX_MANIFEST_BYTES {
        return Some("Das STEP-Strukturmanifest überschreitet die zulässige Größe.".to_owned());
    }
    let value: Value = match serde_json::from_slice(&bytes) {
        Ok(value) => value,
        Err(_) => return Some("Das STEP-Strukturmanifest enthält ungültiges JSON.".to_owned()),
    };
    validate(&value).err()
}

fn validate(value: &Value) -> Result<(), String> {
    let object = value
        .as_object()
        .ok_or_else(|| "Das STEP-Strukturmanifest ist kein Objekt.".to_owned())?;
    if object.get("contractVersion").and_then(Value::as_i64) != Some(1) {
        return Err("Die Version des STEP-Strukturmanifests wird nicht unterstützt.".to_owned());
    }
    let definitions = object
        .get("definitions")
        .and_then(Value::as_array)
        .ok_or_else(|| "Das STEP-Strukturmanifest enthält keine Definitionen.".to_owned())?;
    let mut definition_ids = HashSet::new();
    for definition in definitions {
        let id = bounded_string(definition.get("id"), 160).ok_or_else(|| {
            "Das STEP-Strukturmanifest enthält eine ungültige Definition.".to_owned()
        })?;
        if !definition_ids.insert(id) {
            return Err("Das STEP-Strukturmanifest enthält doppelte Definitionen.".to_owned());
        }
    }
    let roots = object
        .get("roots")
        .and_then(Value::as_array)
        .ok_or_else(|| "Das STEP-Strukturmanifest enthält keine Wurzelknoten.".to_owned())?;
    let mut pending: Vec<(&Value, usize)> = roots.iter().map(|node| (node, 0)).collect();
    let mut node_ids = HashSet::new();
    while let Some((node, depth)) = pending.pop() {
        if depth > MAX_DEPTH || node_ids.len() >= MAX_NODES {
            return Err("Das STEP-Strukturmanifest überschreitet die Strukturgrenzen.".to_owned());
        }
        let id = bounded_string(node.get("id"), 160).ok_or_else(|| {
            "Das STEP-Strukturmanifest enthält einen ungültigen Knoten.".to_owned()
        })?;
        if !node_ids.insert(id) {
            return Err("Das STEP-Strukturmanifest enthält doppelte Knoten-IDs.".to_owned());
        }
        let definition = bounded_string(node.get("definition"), 160).ok_or_else(|| {
            "Das STEP-Strukturmanifest enthält einen ungültigen Knoten.".to_owned()
        })?;
        if !definition_ids.contains(definition) {
            return Err(
                "Ein STEP-Strukturknoten verweist auf eine unbekannte Definition.".to_owned(),
            );
        }
        let transform = node.get("transform").and_then(Value::as_array);
        if transform.is_none_or(|items| {
            items.len() != 16
                || items
                    .iter()
                    .any(|item| item.as_f64().is_none_or(|number| !number.is_finite()))
        }) {
            return Err(
                "Ein STEP-Strukturknoten enthält eine ungültige Transformation.".to_owned(),
            );
        }
        let children = node
            .get("children")
            .and_then(Value::as_array)
            .ok_or_else(|| {
                "Das STEP-Strukturmanifest enthält einen ungültigen Knoten.".to_owned()
            })?;
        pending.extend(children.iter().map(|child| (child, depth + 1)));
    }
    if object.get("transformConvention").and_then(Value::as_str) != Some("row-major, parent-local")
    {
        return Err(
            "Die Transformationskonvention des STEP-Strukturmanifests wird nicht unterstützt."
                .to_owned(),
        );
    }
    if object.get("colorSpace").and_then(Value::as_str) != Some("sRGB") {
        return Err("Der Farbraum des STEP-Strukturmanifests wird nicht unterstützt.".to_owned());
    }
    Ok(())
}

fn bounded_string(value: Option<&Value>, max: usize) -> Option<&str> {
    value
        .and_then(Value::as_str)
        .filter(|text| !text.is_empty() && text.chars().count() <= max)
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::validate;

    #[test]
    fn rejects_legacy_transform_conventions() {
        for convention in [serde_json::Value::Null, json!("column-major")] {
            let value = json!({
                "contractVersion":1,"transformConvention":convention,"colorSpace":"sRGB",
                "definitions":[],"roots":[]
            });
            assert!(validate(&value).is_err(), "legacy convention accepted");
        }
    }

    #[test]
    fn rejects_legacy_color_spaces() {
        for color in [serde_json::Value::Null, json!("linear")] {
            let value = json!({
                "contractVersion":1,"transformConvention":"row-major, parent-local","colorSpace":color,
                "definitions":[],"roots":[]
            });
            assert!(validate(&value).is_err(), "legacy color space accepted");
        }
    }

    #[test]
    fn accepts_current_conventions() {
        let value = json!({
            "contractVersion":1,"transformConvention":"row-major, parent-local","colorSpace":"sRGB",
            "definitions":[],"roots":[]
        });
        assert!(validate(&value).is_ok());
    }

    #[test]
    fn rejects_duplicate_node_ids() {
        let node = |definition: &str| {
            json!({
                "id":"node-1","definition":definition,"transform":[1,0,0,0,0,1,0,0,0,0,1,0,0,0,0,1],"children":[]
            })
        };
        let value = json!({
            "contractVersion":1,
            "definitions":[{"id":"part"}],
            "roots":[node("part"),node("part")]
        });
        assert_eq!(
            validate(&value).expect_err("duplicate must fail"),
            "Das STEP-Strukturmanifest enthält doppelte Knoten-IDs."
        );
    }

    #[test]
    fn rejects_duplicate_node_ids_across_nested_branches() {
        let leaf = |definition: &str| {
            json!({
                "id":"legacy-label","definition":definition,"transform":[1,0,0,0,0,1,0,0,0,0,1,0,0,0,0,1],"children":[]
            })
        };
        let value = json!({
            "contractVersion":1,
            "definitions":[{"id":"assembly"},{"id":"part"}],
            "roots":[{
                "id":"root","definition":"assembly","transform":[1,0,0,0,0,1,0,0,0,0,1,0,0,0,0,1],
                "children":[leaf("part"),{
                    "id":"branch","definition":"assembly","transform":[1,0,0,0,0,1,0,0,0,0,1,0,0,0,0,1],
                    "children":[leaf("part")]
                }]
            }]
        });
        assert_eq!(
            validate(&value).expect_err("nested duplicate must fail"),
            "Das STEP-Strukturmanifest enthält doppelte Knoten-IDs."
        );
    }

    #[test]
    fn rejects_nodes_with_missing_definition_references() {
        let value = json!({
            "contractVersion":1,
            "definitions":[{"id":"known"}],
            "roots":[{
                "id":"node-1","definition":"missing","transform":[1,0,0,0,0,1,0,0,0,0,1,0,0,0,0,1],"children":[]
            }]
        });
        assert_eq!(
            validate(&value).expect_err("missing definition must fail"),
            "Ein STEP-Strukturknoten verweist auf eine unbekannte Definition."
        );
    }
}
