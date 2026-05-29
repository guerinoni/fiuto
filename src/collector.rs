use oas3::Spec;
use oas3::spec::{ObjectOrReference, ObjectSchema, Operation, RequestBody, Schema};

/// Op is the struct that represents an operation in the `OpenAPI` spec.
#[derive(Clone)]
pub struct Op {
    pub path: String,
    pub method: String,
    pub operation: Operation,
    pub payload: Option<ObjectSchema>,
}

pub fn collect_gets(spec: &Spec) -> Vec<Op> {
    let Some(paths) = &spec.paths else {
        return vec![];
    };

    paths
        .iter()
        .filter_map(|(path, item)| {
            let op = item.get.as_ref()?;
            if op.deprecated.unwrap_or(false) {
                return None;
            }
            Some(Op {
                path: path.clone(),
                method: "GET".to_owned(),
                operation: op.clone(),
                payload: None,
            })
        })
        .collect()
}

pub fn collect_post(spec: &Spec) -> Vec<Op> {
    let Some(paths) = &spec.paths else {
        return vec![];
    };

    let mut ops: Vec<Op> = paths
        .iter()
        .filter_map(|(path, item)| {
            let op = item.post.as_ref()?;
            if op.deprecated.unwrap_or(false) {
                return None;
            }

            let req_body = resolve_request_body(op.request_body.as_ref()?, spec)?;
            // FIXME: in long term this should be required? :)
            if !req_body.content.contains_key("application/json") {
                return None;
            }

            Some(Op {
                path: path.clone(),
                method: "POST".to_owned(),
                operation: op.clone(),
                payload: None,
            })
        })
        .collect();

    populate_payload(&mut ops, spec);

    ops
}

/// Resolves a `RequestBody`, following a `$ref` when needed.
fn resolve_request_body(
    req_body: &ObjectOrReference<RequestBody>,
    spec: &Spec,
) -> Option<RequestBody> {
    req_body.resolve(spec).ok()
}

fn populate_payload(ops: &mut [Op], spec: &Spec) {
    for o in ops {
        let Some(req) = o.operation.request_body.as_ref() else {
            continue;
        };

        let Some(req) = resolve_request_body(req, spec) else {
            continue;
        };

        let Some(media_type) = req.content.get("application/json") else {
            continue;
        };

        let Some(schema) = &media_type.schema else {
            tracing::warn!("no schema for request {}", o.path);
            continue;
        };

        match resolve_object_schema(schema, spec) {
            Ok(obj) => o.payload = Some(obj),
            Err(e) => tracing::warn!("cannot resolve payload schema for {}: {e}", o.path),
        }
    }
}

/// Resolves a `Schema` (inline or `$ref`) down to a concrete `ObjectSchema`.
pub fn resolve_object_schema(schema: &Schema, spec: &Spec) -> Result<ObjectSchema, String> {
    match schema.resolve(spec).map_err(|e| e.to_string())? {
        Schema::Object(obj_ref) => match *obj_ref {
            ObjectOrReference::Object(obj) => Ok(obj),
            ObjectOrReference::Ref { ref_path, .. } => {
                Err(format!("unresolved reference: {ref_path}"))
            }
        },
        Schema::Boolean(_) => Err("boolean schema is not supported".to_owned()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::parse_openapi;

    #[test]
    fn scan_get() {
        let s = std::include_str!("./testdata/get_info.yml");
        let spec = parse_openapi(s).unwrap();
        let gets = collect_gets(&spec);
        assert_eq!(gets.len(), 1);
    }

    #[test]
    fn scan_post() {
        let s = std::include_str!("./testdata/post_login.yml");
        let spec = parse_openapi(s).unwrap();
        let posts = collect_post(&spec);
        assert_eq!(posts.len(), 1);

        let f = posts.first().unwrap();
        assert_eq!(f.path, "/api/v1/login");
        assert!(f.payload.is_some());
    }

    #[test]
    fn skip_deprecated() {
        {
            let s = std::include_str!("./testdata/get_info_deprecated.yml");
            let spec = parse_openapi(s).unwrap();
            let gets = collect_gets(&spec);
            assert_eq!(gets.len(), 0);
        }
        {
            let s = std::include_str!("./testdata/post_login_deprecated.yml");
            let spec = parse_openapi(s).unwrap();
            let posts = collect_post(&spec);
            assert_eq!(posts.len(), 0);
        }
    }

    #[test]
    fn post_without_json_content_type_is_filtered() {
        let s = std::include_str!("./testdata/post_non_json_content.yml");
        let spec = parse_openapi(s).unwrap();
        let posts = collect_post(&spec);

        // Should be empty because it doesn't have application/json content type
        assert_eq!(posts.len(), 0);
    }

    #[test]
    fn get_method_is_correctly_identified() {
        let s = std::include_str!("./testdata/get_info.yml");
        let spec = parse_openapi(s).unwrap();
        let gets = collect_gets(&spec);

        assert_eq!(gets.len(), 1);
        let get_op = gets.first().unwrap();
        assert_eq!(get_op.method, "GET");
        assert_eq!(get_op.path, "/api/v1/org/info");
        assert!(get_op.payload.is_none());
    }

    #[test]
    fn post_method_is_correctly_identified() {
        let s = std::include_str!("./testdata/post_login.yml");
        let spec = parse_openapi(s).unwrap();
        let posts = collect_post(&spec);

        assert_eq!(posts.len(), 1);
        let post_op = posts.first().unwrap();
        assert_eq!(post_op.method, "POST");
        assert_eq!(post_op.path, "/api/v1/login");
    }

    #[test]
    fn populate_payload_resolves_references() {
        let s = std::include_str!("./testdata/post_login.yml");
        let spec = parse_openapi(s).unwrap();
        let posts = collect_post(&spec);

        let post_op = posts.first().unwrap();
        // Payload should be populated from the $ref
        assert!(post_op.payload.is_some());
    }

    #[test]
    fn request_body_reference_is_resolved() {
        let s = std::include_str!("./testdata/post_login_request_body_ref.yml");
        let spec = parse_openapi(s).unwrap();
        let posts = collect_post(&spec);

        assert_eq!(posts.len(), 1);
        let post_op = posts.first().unwrap();
        assert_eq!(post_op.method, "POST");
        assert_eq!(post_op.path, "/api/v1/login");
        // Payload should be resolved through the requestBody $ref
        assert!(post_op.payload.is_some());
    }

    #[test]
    fn spec_without_paths_collects_nothing() {
        // single_server.yml declares `paths:` as empty.
        let s = std::include_str!("./testdata/single_server.yml");
        let spec = parse_openapi(s).unwrap();

        assert_eq!(collect_gets(&spec).len(), 0);
        assert_eq!(collect_post(&spec).len(), 0);
    }

    #[test]
    fn get_only_spec_has_no_posts() {
        let s = std::include_str!("./testdata/get_info.yml");
        let spec = parse_openapi(s).unwrap();

        assert_eq!(collect_gets(&spec).len(), 1);
        assert_eq!(collect_post(&spec).len(), 0);
    }

    #[test]
    fn post_only_spec_has_no_gets() {
        let s = std::include_str!("./testdata/post_login.yml");
        let spec = parse_openapi(s).unwrap();

        assert_eq!(collect_gets(&spec).len(), 0);
        assert_eq!(collect_post(&spec).len(), 1);
    }

    #[test]
    fn multi_endpoint_spec_collects_both_methods() {
        let s = std::include_str!("./testdata/multi_endpoint.yml");
        let spec = parse_openapi(s).unwrap();

        let gets = collect_gets(&spec);
        let posts = collect_post(&spec);

        assert_eq!(gets.len(), 1);
        assert_eq!(gets.first().unwrap().path, "/api/v1/org/info");
        assert_eq!(posts.len(), 1);
        assert_eq!(posts.first().unwrap().path, "/api/v1/login");
        assert!(posts.first().unwrap().payload.is_some());
    }

    #[test]
    fn post_payload_with_only_object_level_example_has_no_leaves() {
        // Properties carry no per-property example, only the object carries one;
        // the digger consumes per-property examples, so payload still resolves
        // but yields no usable leaf values downstream.
        let s = std::include_str!("./testdata/post_login_obj_example.yml");
        let spec = parse_openapi(s).unwrap();
        let posts = collect_post(&spec);

        assert_eq!(posts.len(), 1);
        assert!(posts.first().unwrap().payload.is_some());
    }
}
