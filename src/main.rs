use std::vec;

#[tokio::main]
async fn main() {
    tracing_subscriber::fmt::init();

    tracing::info!("fiuto v0.1.0 starting...");

    let file_path = match std::env::args().nth(1) {
        Some(arg) => arg,
        None => {
            tracing::error!("No file path provided, exiting...");
            std::process::exit(1);
        }
    };

    if file_path == "--help" || file_path == "-h" || file_path == "help" {
        println!("Usage: fiuto <openapi file path>");
        std::process::exit(0);
    }

    let s = match std::fs::read_to_string(file_path) {
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

    // TODO: implement the server selection if any are present
    // openapi_schema.servers.iter().for_each(|s| {
    // tracing::info!("server: {:?}", s.url);
    // });

    let components = openapi_schema.components.unwrap(); // FIXME: what if there are no components?

    let mut posts = collect_post(openapi_schema.paths.clone());
    populate_payload(&mut posts, components);

    let gets = collect_gets(openapi_schema.paths);

    let base_url = "http://127.0.0.1:8000"; // FIXME: make this configurable or take from the openapi file

    let mut all_results = vec![];

    for p in gets {
        let result = exec_operation(p, base_url).await;
        all_results.push(result);
    }

    for p in posts {
        let result = exec_operation(p, base_url).await;
        all_results.push(result);
    }

    for r in all_results {
        let string_results = serde_json::to_string_pretty(&r).unwrap(); // FIXME: handle the error
        println!("{}", string_results);
    }
}

async fn exec_operation(op: Op, base_url: &str) -> Vec<CallResult> {
    match op.method.as_str() {
        "GET" => drill_get_endpoint(base_url, &op.path).await,
        "POST" => {
            let s = op.payload.unwrap(); // FIXME: what if there are no payload?
            let mut props = property_for_schema(&s);
            let combs = create_combination_property(&mut props);
            drill_post_endpoint(base_url, &op.path, combs).await
        }
        _ => vec![],
    }
}

#[derive(Debug, serde::Serialize)]
struct CallResult {
    payload: String,
    path: String,
    status_code: u16,
}

async fn drill_get_endpoint(base_url: &str, path: &str) -> Vec<CallResult> {
    let url = format!("{base_url}{path}");

    let client = reqwest::Client::new();
    let req = client.request(reqwest::Method::GET, url.clone());
    let r = req.build().unwrap(); // TODO: handle the error
    let resp = client.execute(r).await.unwrap(); // TODO: handle the error

    vec![CallResult {
        payload: "".to_owned(),
        path: url.to_string(),
        status_code: resp.status().as_u16(),
    }]
}

async fn drill_post_endpoint(
    base_url: &str,
    path: &str,
    prop_combinations: Vec<Vec<(&String, PropertyField)>>,
) -> Vec<CallResult> {
    let url = format!("{base_url}{path}");

    let client = reqwest::Client::new();

    let mut responses = vec![];

    for properties in prop_combinations {
        let mut paylaod = std::collections::HashMap::new();
        for props in properties {
            paylaod.insert(props.0, props.1.example.unwrap());
        }

        let s = serde_json::to_string(&paylaod).unwrap(); // TODO: handle the error

        let req = client
            .request(reqwest::Method::POST, url.clone()) // TODO: Make method configurable
            .body(s.clone())
            .header("Content-Type", "application/json"); // TODO: Make this configurable
        let r = req.build().unwrap(); // TODO: handle the error
        let resp = client.execute(r).await.unwrap(); // TODO: handle the error

        responses.push(CallResult {
            payload: s,
            path: url.to_string(),
            status_code: resp.status().as_u16(),
        });
    }

    responses
}

fn property_for_schema(
    s: &openapiv3::Schema,
) -> std::collections::HashMap<String, openapiv3::ReferenceOr<Box<openapiv3::Schema>>> {
    let mut properties = std::collections::HashMap::new();

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
                    properties.insert(k.to_owned(), v.to_owned());
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

struct Op {
    path: String,
    method: String,
    operation: openapiv3::Operation,
    payload: Option<openapiv3::Schema>,
}

fn collect_gets(paths: openapiv3::Paths) -> Vec<Op> {
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

fn collect_post(paths: openapiv3::Paths) -> Vec<Op> {
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

        for (c, media_type) in &req.content {
            let schema = match &media_type.schema {
                Some(s) => s,
                None => continue,
            };

            let reference = match schema {
                openapiv3::ReferenceOr::Reference { reference } => reference.clone(),
                openapiv3::ReferenceOr::Item(i) => "".to_owned(),
            };

            if reference.is_empty() {
                tracing::warn!("reference is empty");
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
    properties: &mut std::collections::HashMap<
        String,
        openapiv3::ReferenceOr<Box<openapiv3::Schema>>,
    >,
) -> Vec<Vec<(&String, PropertyField)>> {
    let total_combinations = (1 << properties.len()) - 1;
    let mut combination = vec![];

    for mask in 1..=total_combinations {
        let mut comb = vec![];

        for (i, (name, value)) in properties.iter().enumerate() {
            if (mask & (1 << i)) == 0 {
                continue;
            }

            let v = value.as_item();
            let v = v.unwrap();
            let pf = PropertyField {
                example: v.schema_data.example.clone(),
                nullable: v.schema_data.nullable,
            };

            comb.push((name, pf));
        }

        combination.push(comb);
    }

    combination
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scan_get() {
        let s = std::include_str!("./testdata/get_info.yml");
        let openapi_schema = serde_yaml::from_str(s);
        assert!(openapi_schema.is_ok());

        let openapi_schema: openapiv3::OpenAPI = openapi_schema.unwrap();
        let gets = collect_gets(openapi_schema.paths);
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
        let mut gets = collect_gets(openapi_schema.paths);

        let p = gets.pop().unwrap();
        let result = exec_operation(p, &base_url).await;

        assert_eq!(result.len(), 1);
    }

    #[test]
    fn scan_post() {
        let s = std::include_str!("./testdata/post_login.yml");
        let openapi_schema = serde_yaml::from_str(s);
        assert!(openapi_schema.is_ok());

        let openapi_schema: openapiv3::OpenAPI = openapi_schema.unwrap();
        let posts = collect_post(openapi_schema.paths);
        assert_eq!(posts.len(), 1);
        assert_eq!(posts.first().unwrap().path, "/api/v1/login");
    }

    #[test]
    fn check_post_payload() {
        let s = std::include_str!("./testdata/post_login.yml");
        let openapi_schema: openapiv3::OpenAPI = serde_yaml::from_str(s).unwrap();
        let mut posts = collect_post(openapi_schema.paths);
        populate_payload(&mut posts, openapi_schema.components.unwrap());
        let f = posts.first().unwrap();
        assert_ne!(f.payload, None);

        let s = f.payload.clone();
        let s = s.unwrap();
        let mut props = property_for_schema(&s);

        assert_ne!(props.len(), 0);

        let combs = create_combination_property(&mut props);
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
        let mut posts = collect_post(openapi_schema.paths);
        populate_payload(&mut posts, openapi_schema.components.unwrap());

        let p = posts.pop().unwrap();
        let result = exec_operation(p, &base_url).await;

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
