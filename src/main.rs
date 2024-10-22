use clap::Parser;

#[derive(Parser, Debug)]
#[command(version, about, long_about = None)]
struct Args {
    openapi_file: String,

    #[clap(long, short)]
    base_url: Option<String>,
}

#[tokio::main]
async fn main() {
    tracing_subscriber::fmt::init();

    let args = Args::parse();

    let s = match std::fs::read_to_string(args.openapi_file) {
        Ok(s) => s,
        Err(e) => {
            tracing::error!("Error reading file: {:?}", e);
            std::process::exit(1);
        }
    };

    let openapi_schema: openapiv3::OpenAPI = match serde_yaml::from_str(&s) {
        Ok(s) => s,
        Err(e) => {
            tracing::error!("Error parsing yaml: {:?}", e);
            std::process::exit(1);
        }
    };

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

    let base_url = match args.base_url {
        Some(b) => b,
        None => base_url,
    };

    let mut posts = collect_post(&openapi_schema.paths);
    populate_payload(&mut posts, components);

    let gets = collect_gets(&openapi_schema.paths);

    let mut operations = vec![];
    operations.extend_from_slice(gets.as_slice());
    operations.extend_from_slice(posts.as_slice());

    let mut all_results = vec![];

    for p in operations {
        let result = exec_operation(p, &base_url).await;
        match result {
            Ok(r) => all_results.push(r),
            Err(e) => {
                tracing::error!("Error executing operation: {:?}", e);
                break;
            }
        }
    }

    for r in all_results {
        let string_results = serde_json::to_string_pretty(&r).unwrap(); // FIXME: handle the error
        println!("{}", string_results);
    }
}

async fn exec_operation(op: Op, base_url: &str) -> Result<Vec<CallResult>, reqwest::Error> {
    match op.method.as_str() {
        "GET" => drill_get_endpoint(base_url, &op.path).await,
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
            drill_post_endpoint(base_url, &op.path, combs).await
        }
        _ => {
            tracing::warn!("Unsupported method: {}", op.method);
            Ok(vec![])
        }
    }
}

#[derive(Debug, serde::Serialize)]
struct CallResult {
    payload: String,
    path: String,
    status_code: u16,
}

async fn drill_get_endpoint(base_url: &str, path: &str) -> Result<Vec<CallResult>, reqwest::Error> {
    let url = format!("{base_url}{path}");

    let client = reqwest::Client::new();
    let req = client.request(reqwest::Method::GET, url.clone());
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

        let req = client
            .request(reqwest::Method::POST, url.clone()) // TODO: Make method configurable
            .body(s.clone())
            .header("Content-Type", "application/json"); // TODO: Make this configurable
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
            }

            return properties;
        }
        None => {
            tracing::warn!(
                "No paylaod object example found in the schema, using example of single property"
            );
        }
    }

    match &s.schema_kind {
        openapiv3::SchemaKind::Type(t) => match t {
            openapiv3::Type::String(s) => {
                println!("string: {:?}", s);
            }
            openapiv3::Type::Number(n) => {
                println!("number: {:?}", n);
            }
            openapiv3::Type::Object(o) => {
                for (k, v) in &o.properties {
                    let v = v.as_item();
                    let v = v.unwrap();
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
        },
        _ => {}
    }

    properties
}

#[derive(Clone)]
struct Op {
    path: String,
    method: String,
    operation: openapiv3::Operation,
    payload: Option<openapiv3::Schema>,
}

fn collect_gets(paths: &openapiv3::Paths) -> Vec<Op> {
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
        .map(|p| Op {
            path: p.0,
            method: "GET".to_owned(),
            operation: p.1.get.unwrap(),
            payload: None,
        })
        .collect()
}

fn collect_post(paths: &openapiv3::Paths) -> Vec<Op> {
    paths
        .iter()
        .map(|p| {
            let pp = p.0.to_owned();
            let i = p.1.to_owned();
            let i = i.as_item();
            let i = i.unwrap();
            (pp, i.clone())
        })
        .filter(|p| p.1.post.is_some())
        .map(|p| Op {
            path: p.0,
            method: "POST".to_owned(),
            operation: p.1.post.unwrap(),
            payload: None,
        })
        .collect()
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
    nullable: bool,
}

fn create_combination_property(
    properties: &mut std::collections::HashMap<String, PropertyField>,
) -> Vec<Vec<(&String, &PropertyField)>> {
    let total_combinations = (1 << properties.len()) - 1;
    let mut combination = vec![];

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

    #[tokio::test]
    async fn run_get_request() {
        let state = std::sync::Arc::new(AppState {});
        let app = axum::Router::new()
            .route("/api/v1/org/info", axum::routing::get(info))
            .with_state(state);

        let listener = tokio::net::TcpListener::bind("0.0.0.0:0").await.unwrap();
        println!("listening on: {}", listener.local_addr().unwrap());
        let base_url = format!("http://{}", listener.local_addr().unwrap());
        tokio::spawn(async move {
            axum::serve(listener, app).await.unwrap();
        });

        let s = std::include_str!("./testdata/get_info.yml");
        let openapi_schema: openapiv3::OpenAPI = serde_yaml::from_str(s).unwrap();
        let mut gets = collect_gets(&openapi_schema.paths);

        let p = gets.pop().unwrap();
        let result = exec_operation(p, &base_url).await;

        assert!(result.is_ok());

        let result = result.unwrap();

        assert_eq!(result.len(), 1);
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
        assert_eq!(combs.len(), 7);
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
        assert_eq!(combs.len(), 7);
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
        assert_eq!(combs.len(), 7);
    }

    #[tokio::test]
    async fn run_post_request() {
        let state = std::sync::Arc::new(AppState {});
        let app = axum::Router::new()
            .route("/api/v1/login", axum::routing::post(login_handler))
            .with_state(state);

        let listener = tokio::net::TcpListener::bind("0.0.0.0:0").await.unwrap();
        println!("listening on: {}", listener.local_addr().unwrap());
        let base_url = format!("http://{}", listener.local_addr().unwrap());
        tokio::spawn(async move {
            axum::serve(listener, app).await.unwrap();
        });

        let s = std::include_str!("./testdata/post_login.yml");
        let openapi_schema: openapiv3::OpenAPI = serde_yaml::from_str(s).unwrap();
        let mut posts = collect_post(&openapi_schema.paths);
        populate_payload(&mut posts, openapi_schema.components.unwrap());

        let p = posts.pop().unwrap();
        let result = exec_operation(p, &base_url).await;

        assert!(result.is_ok());

        let result = result.unwrap();

        assert_eq!(result.len(), 7);
    }

    #[derive(serde::Deserialize, serde::Serialize, Debug)]
    struct LoginRequest {
        email: String,
        password: String,
        org: String,
    }

    #[derive(Clone)]
    struct AppState {}

    async fn login_handler(
        axum::extract::State(state): axum::extract::State<std::sync::Arc<AppState>>,
        axum::Json(payload): axum::Json<LoginRequest>,
    ) -> axum::Json<String> {
        println!("Received login request: {:?}", payload);
        axum::Json(format!(
            "User {} from org {} is trying to login",
            payload.email, payload.org
        ))
    }

    async fn info() -> axum::Json<&'static str> {
        axum::Json("Hello, World!")
    }
}
