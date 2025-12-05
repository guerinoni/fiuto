/// Op is the struct that represents an operation in the `OpenAPI` spec.
#[derive(Clone)]
pub struct Op {
    pub path: String,
    pub method: String,
    pub operation: openapiv3::Operation,
    pub payload: Option<openapiv3::Schema>,
}

pub fn collect_gets(paths: &openapiv3::Paths) -> Vec<Op> {
    paths
        .iter()
        .map(|p| {
            let pp = p.0.to_owned();
            let i = p.1.to_owned();
            let i = i.as_item();
            let i = i.unwrap();
            (pp, i.clone())
        })
        .filter(|p| p.1.get.is_some())
        .filter(|p| !p.1.get.as_ref().unwrap().deprecated)
        .map(|p| Op {
            path: p.0,
            method: "GET".to_owned(),
            operation: p.1.get.unwrap(),
            payload: None,
        })
        .collect()
}

pub fn collect_post(paths: &openapiv3::Paths, components: &openapiv3::Components) -> Vec<Op> {
    let mut p = paths
        .iter()
        .map(|p| {
            let pp = p.0.to_owned();
            let i = p.1.to_owned();
            let i = i.as_item();
            let i = i.unwrap();
            (pp, i.clone())
        })
        .filter(|p| p.1.post.is_some())
        .filter(|p| !p.1.post.as_ref().unwrap().deprecated)
        .map(|p| {
            let post = p.1.post.unwrap();
            let path = p.0;
            (path, post)
        })
        .filter(|p| p.1.request_body.is_some())
        .filter(|p| {
            let req_body = p.1.request_body.as_ref().unwrap();
            let req_body = req_body.as_item().unwrap();
            req_body
                .content
                .iter()
                .any(|(k, _)| k == "application/json")
        })
        .map(|p| Op {
            path: p.0,
            method: "POST".to_owned(),
            operation: p.1,
            payload: None,
        })
        .collect();

    populate_payload(&mut p, components);

    p
}

fn populate_payload(op: &mut Vec<Op>, components: &openapiv3::Components) {
    for o in op {
        let Some(req) = &o.operation.request_body else {
            continue;
        };

        let Some(req) = req.as_item() else { continue };

        for (_, media_type) in &req.content {
            let Some(schema) = &media_type.schema else {
                continue;
            };

            let reference = match schema {
                openapiv3::ReferenceOr::Reference { reference } => reference.clone(),
                openapiv3::ReferenceOr::Item(_) => String::new(),
            };

            if reference.is_empty() {
                tracing::warn!("reference is empty for request {}", o.path);
                continue;
            }

            let reference = reference.trim_start_matches("#/components/schemas/");
            let Some(schema) = components.schemas.get(reference) else {
                continue;
            };

            let ss = schema.as_item();
            o.payload = ss.cloned();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scan_get() {
        let s = std::include_str!("./testdata/get_info.yml");
        let openapi_schema = serde_yaml_bw::from_str(s);
        assert!(openapi_schema.is_ok());

        let openapi_schema: openapiv3::OpenAPI = openapi_schema.unwrap();
        let gets = collect_gets(&openapi_schema.paths);
        assert_eq!(gets.len(), 1);
    }

    #[test]
    fn scan_post() {
        let s = std::include_str!("./testdata/post_login.yml");
        let openapi_schema = serde_yaml_bw::from_str(s);
        assert!(openapi_schema.is_ok());

        let openapi_schema: openapiv3::OpenAPI = openapi_schema.unwrap();
        let posts = collect_post(&openapi_schema.paths, &openapi_schema.components.unwrap());
        assert_eq!(posts.len(), 1);

        let f = posts.first().unwrap();
        assert_eq!(f.path, "/api/v1/login");
        assert_ne!(f.payload, None);
    }

    #[test]
    fn skip_deprecated() {
        {
            let s = std::include_str!("./testdata/get_info_deprecated.yml");
            let openapi_schema = serde_yaml_bw::from_str(s);
            assert!(openapi_schema.is_ok());

            let openapi_schema: openapiv3::OpenAPI = openapi_schema.unwrap();
            let gets = collect_gets(&openapi_schema.paths);
            assert_eq!(gets.len(), 0);
        }
        {
            let s = std::include_str!("./testdata/post_login_deprecated.yml");
            let openapi_schema = serde_yaml_bw::from_str(s);
            assert!(openapi_schema.is_ok());

            let openapi_schema: openapiv3::OpenAPI = openapi_schema.unwrap();
            let posts = collect_post(&openapi_schema.paths, &openapi_schema.components.unwrap());
            assert_eq!(posts.len(), 0);
        }
    }

    #[test]
    fn post_without_json_content_type_is_filtered() {
        let s = std::include_str!("./testdata/post_non_json_content.yml");
        let openapi_schema: openapiv3::OpenAPI = serde_yaml_bw::from_str(s).unwrap();
        let posts = collect_post(&openapi_schema.paths, &openapi_schema.components.unwrap());

        // Should be empty because it doesn't have application/json content type
        assert_eq!(posts.len(), 0);
    }

    #[test]
    fn get_method_is_correctly_identified() {
        let s = std::include_str!("./testdata/get_info.yml");
        let openapi_schema: openapiv3::OpenAPI = serde_yaml_bw::from_str(s).unwrap();
        let gets = collect_gets(&openapi_schema.paths);

        assert_eq!(gets.len(), 1);
        let get_op = gets.first().unwrap();
        assert_eq!(get_op.method, "GET");
        assert_eq!(get_op.path, "/api/v1/org/info");
        assert!(get_op.payload.is_none());
    }

    #[test]
    fn post_method_is_correctly_identified() {
        let s = std::include_str!("./testdata/post_login.yml");
        let openapi_schema: openapiv3::OpenAPI = serde_yaml_bw::from_str(s).unwrap();
        let posts = collect_post(&openapi_schema.paths, &openapi_schema.components.unwrap());

        assert_eq!(posts.len(), 1);
        let post_op = posts.first().unwrap();
        assert_eq!(post_op.method, "POST");
        assert_eq!(post_op.path, "/api/v1/login");
    }

    #[test]
    fn populate_payload_resolves_references() {
        let s = std::include_str!("./testdata/post_login.yml");
        let openapi_schema: openapiv3::OpenAPI = serde_yaml_bw::from_str(s).unwrap();
        let posts = collect_post(&openapi_schema.paths, &openapi_schema.components.unwrap());

        let post_op = posts.first().unwrap();
        // Payload should be populated from the $ref
        assert!(post_op.payload.is_some());
    }
}
