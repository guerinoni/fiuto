use serde_yaml_bw::Value;

/// Parse an `OpenAPI` document from YAML text.
///
/// `oas3` targets `OpenAPI` 3.1.x (JSON Schema 2020-12).
/// Most 3.0.x specs still load through it unchanged, with one exception: 3.0 writes
/// `exclusiveMinimum`/`exclusiveMaximum` as booleans, while 3.1 expects numbers,
/// so the boolean form fails to deserialize. We strip those boolean flags before
/// handing the document to `oas3` so older specs keep parsing. The flags only
/// affect numeric bounds validation, which fiuto does not use when drilling
/// payloads, so dropping them is lossless for our purposes.
///
/// # Errors
///
/// Returns a human-readable message (with the YAML line/column when available)
/// if the document is not valid YAML or does not match the `OpenAPI` structure.
pub fn parse_openapi(input: &str) -> Result<oas3::Spec, String> {
    let mut doc: Value = serde_yaml_bw::from_str(input).map_err(|e| format_yaml_error(&e))?;

    let mut changed = false;
    downlevel_30(&mut doc, &mut changed);

    // Only re-serialize when we actually patched something, so the untouched
    // common case feeds the original text straight to oas3.
    if changed {
        let patched = serde_yaml_bw::to_string(&doc).map_err(|e| e.to_string())?;
        oas3::from_yaml(patched).map_err(|e| format!("invalid OpenAPI document: {e}"))
    } else {
        oas3::from_yaml(input).map_err(|e| format!("invalid OpenAPI document: {e}"))
    }
}

fn format_yaml_error(e: &serde_yaml_bw::Error) -> String {
    e.location().map_or_else(
        || e.to_string(),
        |loc| format!("{e} (line {}, column {})", loc.line(), loc.column()),
    )
}

/// Strip 3.0-only boolean `exclusiveMinimum`/`exclusiveMaximum` flags so the
/// document deserializes under the 3.1 schema model.
fn downlevel_30(value: &mut Value, changed: &mut bool) {
    match value {
        Value::Sequence(items) => items.iter_mut().for_each(|v| downlevel_30(v, changed)),
        Value::Mapping(map) => {
            for (_, child) in map.iter_mut() {
                downlevel_30(child, changed);
            }
            for key in ["exclusiveMinimum", "exclusiveMaximum"] {
                if matches!(map.get(key), Some(Value::Bool(..))) {
                    map.remove(key);
                    *changed = true;
                }
            }
        }
        _ => {}
    }
}

#[cfg(test)]
mod tests {
    use super::parse_openapi;

    fn spec_with_field(field_body: &str) -> String {
        format!(
            r#"
openapi: 3.1.0
info:
  title: t
  version: "1.0"
paths: {{}}
components:
  schemas:
    Thing:
      type: object
      properties:
        field:
          {field_body}
"#
        )
    }

    #[test]
    fn nullable_type_list_is_accepted() {
        // 3.1 nullable form: type as a list including "null".
        let spec = spec_with_field(r#"type: [integer, "null"]"#);
        let api = parse_openapi(&spec).expect("3.1 nullable type should parse");
        assert_eq!(api.openapi, "3.1.0");
    }

    #[test]
    fn plain_single_type_still_parses() {
        let spec = spec_with_field("type: integer");
        parse_openapi(&spec).expect("plain type should parse");
    }

    #[test]
    fn boolean_exclusive_minimum_from_30_is_tolerated() {
        // 3.0 wrote exclusiveMinimum as a boolean; oas3 wants a number. The
        // shim drops the flag so the doc still loads.
        let spec = spec_with_field(
            "type: integer\n          minimum: 1\n          exclusiveMinimum: true",
        );
        parse_openapi(&spec).expect("3.0 boolean exclusiveMinimum should be tolerated");
    }

    #[test]
    fn invalid_yaml_reports_location() {
        let err = parse_openapi("openapi: : :").expect_err("garbage should fail");
        assert!(err.contains("line"), "error should mention a line: {err}");
    }
}
