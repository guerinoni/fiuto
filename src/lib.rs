mod collector;
#[derive(Debug, serde::Serialize)]
pub struct CallResult {
    payload: String,
    path: String,
    pub status_code: u16,
}

pub async fn do_it(
    openapi_schema: openapiv3::OpenAPI,
    url: Option<String>,
    jwt: Option<String>,
) -> Result<Vec<Vec<CallResult>>, reqwest::Error> {
    tracing::info!("openapi version: {}", openapi_schema.openapi);

    let components = match openapi_schema.components {
        Some(c) => c,
        None => {
            tracing::error!("No components found in the openapi schema");
            std::process::exit(1);
        }
    };

    let base_url = match openapi_schema.servers.first() {
        Some(s) => s.url.clone(),
        None => {
            tracing::error!("No servers found in the openapi schema");
            std::process::exit(1);
        }
    };

    let base_url = match url {
        Some(b) => b,
        None => base_url,
    };

    let jwt_name = get_jwt_token(&components);

    let posts = collector::collect_post(&openapi_schema.paths, &components);
    let gets = collector::collect_gets(&openapi_schema.paths);

    let mut operations = vec![];
    operations.extend_from_slice(gets.as_slice());
    operations.extend_from_slice(posts.as_slice());

    let mut all_results = vec![];

    for p in operations {
        let result = exec_operation(p.clone(), &base_url, (jwt_name.clone(), jwt.clone())).await;
        match result {
            Ok(r) => all_results.push(r),
            Err(e) => {
                tracing::error!("Error executing operation: {:?}", e);
                return Err(e);
            }
        }
    }

    Ok(all_results)
}

fn get_jwt_token(components: &openapiv3::Components) -> Option<String> {
    components
        .security_schemes
        .iter()
        .filter_map(|(k, v)| {
            let v = v.as_item();
            v.map(|scheme| (k.clone(), scheme.clone()))
        })
        .find_map(|(k, v)| match v {
            openapiv3::SecurityScheme::HTTP {
                scheme, // FIXME: do we need to validate other fields here?
                ..
            } if scheme.to_lowercase() == "bearer" => Some(k),
            _ => None,
        })
}

fn populate_payload(op: &mut Vec<Op>, components: openapiv3::Components) {
    for o in op {
        let req = match &o.operation.request_body {
            Some(r) => r,
            None => continue,
        };

        let req = match req.as_item() {
            Some(r) => r,
            None => continue,
        };

        for (_, media_type) in &req.content {
            let schema = match &media_type.schema {
                Some(s) => s,
                None => continue,
            };

            let reference = match schema {
                openapiv3::ReferenceOr::Reference { reference } => reference.clone(),
                openapiv3::ReferenceOr::Item(_) => "".to_owned(),
            };

            if reference.is_empty() {
                tracing::warn!("reference is empty for request {}", o.path);
                continue;
            }

            let reference = reference.trim_start_matches("#/components/schemas/");
            let schema = match components.schemas.get(reference) {
                Some(s) => s,
                None => continue,
            };

            let ss = schema.as_item();
            o.payload = ss.cloned();
        }
    }
}

#[derive(Debug)]
struct PropertyField {
    example: Option<serde_json::Value>,
    nullable: bool, // TODO: handle combinations with nullable fields
}

fn property_for_schema(s: &openapiv3::Schema) -> std::collections::HashMap<String, PropertyField> {
    let mut properties = std::collections::HashMap::new();

    match &s.schema_data.example {
        Some(e) => {
            for (k, v) in e.as_object().unwrap() {
                let pf = PropertyField {
                    example: Some(v.clone()),
                    nullable: false,
                };

                properties.insert(k.to_owned(), pf);

            return properties;
        }
        None => {
            tracing::warn!(
                "No paylaod object example found in the schema, using example of single property"
            );
        }
    }

    if let openapiv3::SchemaKind::Type(t) = &s.schema_kind {
        match t {
            openapiv3::Type::String(s) => {
                println!("string: {:?}", s);
            }
            openapiv3::Type::Number(n) => {
                println!("number: {:?}", n);
            }
            openapiv3::Type::Object(o) => {
                for (k, v) in &o.properties {
                    let v = v.as_item();
                    let v = match v {
                        Some(v) => v,
                        None => {
                            tracing::warn!("No item found for property {}", k);
                            continue;
                        }
                    };
                    let pf = PropertyField {
                        example: v.schema_data.example.clone(),
                        nullable: v.schema_data.nullable,
                    };

                    properties.insert(k.to_owned(), pf);
                }
            }
            openapiv3::Type::Array(a) => {
                println!("array: {:?}", a);
            }
            openapiv3::Type::Boolean(b) => {
                println!("boolean: {:?}", b);
            }
            openapiv3::Type::Integer(i) => {
                println!("integer: {:?}", i);
            }
        }
    }

    properties
}

async fn exec_operation(
    op: collector::Op,
    base_url: &str,
    (jwt_name, jwt): (Option<String>, Option<String>),
) -> Result<Vec<CallResult>, reqwest::Error> {
    match op.method.as_str() {
        "GET" => {
            drill_get_endpoint(base_url, &op.path, (jwt_name, jwt), op.operation.security).await
        }
        "POST" => {
            let s = match op.payload {
                Some(s) => s,
                None => {
                    tracing::warn!("No payload found for POST {}", op.path);
                    return Ok(vec![]);
                }
            };
            let mut props = property_for_schema(&s);
            let combs = create_combination_property(&mut props);
            drill_post_endpoint(
                base_url,
                &op.path,
                (jwt_name, jwt),
                op.operation.security,
                combs,
            )
            .await
        }
        _ => {
            tracing::warn!("Unsupported method: {}", op.method);
            Ok(vec![])
        }
    }
}

fn create_combination_property(
    properties: &mut std::collections::HashMap<String, PropertyField>,
) -> Vec<Vec<(&String, &PropertyField)>> {
    let total_combinations = (1 << properties.len()) - 1;
    let mut combination = vec![];

    if total_combinations > 0 {
        // generate empty combination
        combination.push(vec![]);
    }

    for mask in 1..=total_combinations {
        let mut comb = vec![];

        for (i, (name, value)) in properties.iter().enumerate() {
            if (mask & (1 << i)) == 0 {
                continue;
            }

            comb.push((name, value));
        }

        combination.push(comb);
    }

    combination
}

async fn drill_get_endpoint(
    base_url: &str,
    path: &str,
    (jwt_name, jwt): (Option<String>, Option<String>),
    security: Option<Vec<openapiv3::SecurityRequirement>>,
) -> Result<Vec<CallResult>, reqwest::Error> {
    let url = format!("{base_url}{path}");

    let client = reqwest::Client::new();
    let mut req = client.request(reqwest::Method::GET, url.clone());

    if let Some(s) = security {
        if jwt.is_some() && jwt_name.is_some() {
            for ss in s.iter() {
                for (k, _) in ss.iter() {
                    let jwt_name = jwt_name.clone().unwrap();
                    let jwt = jwt.clone().unwrap();

                    if k == &jwt_name {
                        req = req.header("Authorization", format!("Bearer {}", jwt));
                    }
                }
            }
        }
    }

    let r = req.build().unwrap(); // TODO: handle the error
    let resp = client.execute(r).await?;

    Ok(vec![CallResult {
        payload: "".to_owned(),
        path: url.to_string(),
        status_code: resp.status().as_u16(),
    }])
}

async fn drill_post_endpoint(
    base_url: &str,
    path: &str,
    (jwt_name, jwt): (Option<String>, Option<String>),
    security: Option<Vec<openapiv3::SecurityRequirement>>,
    prop_combinations: Vec<Vec<(&String, &PropertyField)>>,
) -> Result<Vec<CallResult>, reqwest::Error> {
    let url = format!("{base_url}{path}");

    let client = reqwest::Client::new();

    let mut responses = vec![];

    for properties in prop_combinations {
        let mut paylaod = std::collections::HashMap::new();
        for props in properties {
            let pf = props.1;
            paylaod.insert(
                props.0,
                pf.example.clone().unwrap_or(serde_json::Value::Null),
            );
        }

        let s = serde_json::to_string(&paylaod).unwrap(); // TODO: handle the error

        let mut req = client
            .request(reqwest::Method::POST, url.clone())
            .body(s.clone())
            .header("Content-Type", "application/json"); // TODO: Make this configurable

        tracing::info!("jwt info: {:?} {:?}", jwt_name, jwt);

        if let Some(ref s) = security {
            if jwt.is_some() && jwt_name.is_some() {
                for ss in s.iter() {
                    for (k, _) in ss.iter() {
                        let jwt_name = jwt_name.clone().unwrap();
                        let jwt = jwt.clone().unwrap();

                        if k == &jwt_name {
                            req = req.header("Authorization", format!("Bearer {}", jwt));
                        }
                    }
                }
            }
        }

        let r = req.build().unwrap(); // TODO: handle the error
        let resp = client.execute(r).await?;

        responses.push(CallResult {
            payload: s,
            path: url.to_string(),
            status_code: resp.status().as_u16(),
        });
    }

    Ok(responses)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// This is a fake test to make sure the test suite is setup with tracing.
    #[test]
    fn fake_test() {
        tracing_subscriber::fmt::init();
        assert!(true);
    }

    #[test]
    fn scan_get() {
        let s = std::include_str!("./testdata/get_info.yml");
        let openapi_schema = serde_yaml::from_str(s);
        assert!(openapi_schema.is_ok());

        let openapi_schema: openapiv3::OpenAPI = openapi_schema.unwrap();
        let gets = collect_gets(&openapi_schema.paths);
        assert_eq!(gets.len(), 1);
    }

    #[test]
    fn scan_post() {
        let s = std::include_str!("./testdata/post_login.yml");
        let openapi_schema = serde_yaml::from_str(s);
        assert!(openapi_schema.is_ok());

        let openapi_schema: openapiv3::OpenAPI = openapi_schema.unwrap();
        let posts = collect_post(&openapi_schema.paths);
        assert_eq!(posts.len(), 1);
        assert_eq!(posts.first().unwrap().path, "/api/v1/login");
    }

    #[test]
    fn check_post_payload() {
        let s = std::include_str!("./testdata/post_login.yml");
        let openapi_schema: openapiv3::OpenAPI = serde_yaml::from_str(s).unwrap();
        let mut posts = collect_post(&openapi_schema.paths);
        populate_payload(&mut posts, openapi_schema.components.unwrap());
        let f = posts.first().unwrap();
        assert_ne!(f.payload, None);

        let s = f.payload.clone();
        let s = s.unwrap();
        let mut props = property_for_schema(&s);

        assert_ne!(props.len(), 0);

        assert!(props.contains_key("email"));
        assert!(props.contains_key("password"));
        assert!(props.contains_key("org"));

        let combs = create_combination_property(&mut props);
        assert_eq!(combs.len(), 8);
    }

    #[test]
    fn check_post_payload_full_example_obj() {
        let s = std::include_str!("./testdata/post_login_obj_example.yml");
        let openapi_schema: openapiv3::OpenAPI = serde_yaml::from_str(s).unwrap();
        let mut posts = collect_post(&openapi_schema.paths);
        populate_payload(&mut posts, openapi_schema.components.unwrap());
        let f = posts.first().unwrap();
        assert_ne!(f.payload, None);

        let s = f.payload.clone();
        let s = s.unwrap();
        let mut props = property_for_schema(&s);

        assert_ne!(props.len(), 0);

        assert!(props.contains_key("email"));
        assert!(props.contains_key("password"));
        assert!(props.contains_key("org"));

        let combs: Vec<Vec<(&String, &PropertyField)>> = create_combination_property(&mut props);
        assert_eq!(combs.len(), 8);
    }

    #[test]
    fn check_post_payload_single_example_properties() {
        let s = std::include_str!("./testdata/post_login_properties_example.yml");
        let openapi_schema: openapiv3::OpenAPI = serde_yaml::from_str(s).unwrap();
        let mut posts = collect_post(&openapi_schema.paths);
        populate_payload(&mut posts, openapi_schema.components.unwrap());
        let f = posts.first().unwrap();
        assert_ne!(f.payload, None);

        let s = f.payload.clone();
        let s = s.unwrap();
        let mut props = property_for_schema(&s);

        assert_ne!(props.len(), 0);

        assert!(props.contains_key("email"));
        assert!(props.contains_key("password"));
        assert!(props.contains_key("org"));

        let combs: Vec<Vec<(&String, &PropertyField)>> = create_combination_property(&mut props);
        assert_eq!(combs.len(), 8);
    }

    #[test]
    fn skip_non_json_content() {
        let s = std::include_str!("./testdata/post_non_json_content.yml");
        let openapi_schema = serde_yaml::from_str(s);
        assert!(openapi_schema.is_ok());

        let openapi_schema: openapiv3::OpenAPI = openapi_schema.unwrap();
        let posts = collect_post(&openapi_schema.paths);

        assert_eq!(posts.len(), 0);
    }

    #[test]
    fn find_jwt_token_in_components() {
        let s = std::include_str!("./testdata/get_more_info_with_jwt.yml");
        let openapi_schema = serde_yaml::from_str(s);
        assert!(openapi_schema.is_ok());

        let openapi_schema: openapiv3::OpenAPI = openapi_schema.unwrap();
        let components = openapi_schema.components.unwrap();
        let jwt = get_jwt_token(&components);

        assert!(jwt.is_some());
        assert_eq!(jwt.unwrap(), "bearerAuth");
    }

    #[test]
    fn skip_deprecated() {
        {
            let s = std::include_str!("./testdata/get_info_deprecated.yml");
            let openapi_schema = serde_yaml::from_str(s);
            assert!(openapi_schema.is_ok());

            let openapi_schema: openapiv3::OpenAPI = openapi_schema.unwrap();
            let gets = collect_gets(&openapi_schema.paths);
            assert_eq!(gets.len(), 0);
        }
        {
            let s = std::include_str!("./testdata/post_login_deprecated.yml");
            let openapi_schema = serde_yaml::from_str(s);
            assert!(openapi_schema.is_ok());

            let openapi_schema: openapiv3::OpenAPI = openapi_schema.unwrap();
            let posts = collect_post(&openapi_schema.paths);
            assert_eq!(posts.len(), 0);
        }
    }

    #[test]
    fn check_combinations() {
        {
            // empty generate zero combinations
            let mut hm = std::collections::hash_map::HashMap::new();
            let comb = create_combination_property(&mut hm);
            assert_eq!(comb.len(), 0);
        }
        {
            // single property generate 2 combination, [<property>, <empty>]
            let mut hm = std::collections::hash_map::HashMap::new();
            hm.insert(
                "one".to_string(),
                PropertyField {
                    example: Some(serde_json::Value::Bool(true)),
                    nullable: false,
                },
            );
            let comb = create_combination_property(&mut hm);
            assert_eq!(comb.len(), 2);
            assert_eq!(comb.first().unwrap().len(), 0);
            assert_eq!(comb.last().unwrap().len(), 1);
        }
        {
            // 2 properties generate [<p1,p2>, <p1>, <p2>, <empty>]
            let mut hm = std::collections::hash_map::HashMap::new();
            hm.insert(
                "one".to_string(),
                PropertyField {
                    example: Some(serde_json::Value::Bool(true)),
                    nullable: false,
                },
            );
            hm.insert(
                "two".to_string(),
                PropertyField {
                    example: Some(serde_json::Value::Bool(true)),
                    nullable: false,
                },
            );
            let mut comb = create_combination_property(&mut hm);
            assert_eq!(comb.len(), 4);
            let fourth = comb.pop().unwrap();
            assert_eq!(fourth.len(), 2); // p1, p2
            let third = comb.pop().unwrap();
            assert_eq!(third.len(), 1); // p2
            let second = comb.pop().unwrap();
            assert_eq!(second.len(), 1); // p1
            let first = comb.pop().unwrap();
            assert_eq!(first.len(), 0); // empty
        }
    }
}
