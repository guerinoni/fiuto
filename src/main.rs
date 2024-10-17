#[tokio::main]
async fn main() {
    let file_path = match std::env::args().nth(1) {
        Some(arg) => arg,
        None => {
            eprintln!("Usage: ./fiuto FILE");
            std::process::exit(1);
        }
    };

    let s = match std::fs::read_to_string(file_path) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("Error reading file: {}", e);
            std::process::exit(1);
        }
    };

    tracing::info!("file lines count: {}", s.lines().count());

    let openapi_schema: openapiv3::OpenAPI = serde_yaml::from_str(&s).unwrap();
    tracing::info!("openapi version: {}", openapi_schema.openapi);

    openapi_schema.servers.iter().for_each(|s| {
        tracing::info!("server: {:?}", s.url);
    });

    let components = openapi_schema.components.unwrap();

    let mut results = vec![];

    for (path, path_item) in openapi_schema.paths {
        // println!("path: {}", path);

        let item = if let Some(item) = path_item.as_item() {
            item.to_owned()
        } else {
            continue;
        };

        if let Some(post) = item.post {
            // println!("  post: {:?}", post.operation_id);
            // println!("  request_body: {:?}", post.request_body);
            if let Some(request_body) = post.request_body {
                let req = if let Some(item) = request_body.as_item() {
                    item.to_owned()
                } else {
                    continue;
                };

                let content = req.content;
                for (c, media_type) in content {
                    // println!("    content: {:?}", c);
                    // println!("    media_type: {:?}", media_type);

                    let schema = media_type.schema.unwrap();
                    let reference = match schema {
                        openapiv3::ReferenceOr::Reference { reference } => {
                            // println!("    reference: {:?}", reference);

                            reference
                        }
                        openapiv3::ReferenceOr::Item(i) => {
                            // println!("    item: {:?}", i);
                            "".to_owned()
                        }
                    };

                    if reference != "" {
                        let reference = reference.trim_start_matches("#/components/schemas/");
                        let schema = components.schemas.get(reference).unwrap(); // TODO: handle the error
                        let prop = property_for_schema(schema.as_item().unwrap());
                        let r = send_request(&openapi_schema.servers[0], &path, prop).await;
                        results.push(r);
                    }
                }
            }
        }
    }

    for r in results {
        match r {
            Ok(resp) => {
                println!("good -> {resp:#?}")
            }
            Err(e) => {
                println!("err  -> {e:#?}")
            }
        }
    }
}

async fn send_request(
    server: &openapiv3::Server,
    path: &str,
    properties: std::collections::HashMap<String, openapiv3::ReferenceOr<Box<openapiv3::Schema>>>,
) -> Result<reqwest::Response, reqwest::Error> {
    let url = format!("{}{}", server.url.to_owned(), path);
    let client = reqwest::Client::new();

    let mut paylaod = std::collections::HashMap::new();
    for (name, prop) in properties {
        let item = prop.as_item().unwrap();
        // println!("item: {:#?}", item.schema_data);
        let example = item.schema_data.example.clone().unwrap(); // TODO: handle the error
        paylaod.insert(name, example);
    }

    let s = serde_json::to_string(&paylaod).unwrap(); // TODO: handle the error
                                                      // println!("payload: {:?}", s);
    let req = client
        .request(reqwest::Method::POST, &url)
        .body(s)
        .header("Content-Type", "application/json");
    let r = req.build().unwrap(); // TODO: handle the error
                                  // println!("request: {:?}", r);
    let res = client.execute(r).await;

    res
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
    operation: openapiv3::Operation,
    payload: Option<openapiv3::Schema>,
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

            if reference == "" {
                tracing::info!("reference is empty");
                continue;
            }

            let reference = reference.trim_start_matches("#/components/schemas/");
            let schema = match components.schemas.get(reference) {
                Some(s) => s,
                None => continue,
            };

            let ss = schema.as_item();
            o.payload = match ss {
                Some(s) => Some(s.clone()),
                None => None,
            };
        }
    }
}

struct CallResult {
    resp: reqwest::Response,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn post() {
        let s = std::include_str!("./testdata/spec.yml");
        let openapi_schema = serde_yaml::from_str(s);
        assert!(openapi_schema.is_ok());
        let openapi_schema: openapiv3::OpenAPI = openapi_schema.unwrap();

        let mut posts = collect_post(openapi_schema.paths);
        assert_eq!(posts.len(), 1);

        let f = posts.first().unwrap();
        assert_eq!(f.payload, None);
        assert_eq!(f.path, "/api/v1/login");

        populate_payload(&mut posts, openapi_schema.components.unwrap());
        assert_eq!(posts.len(), 1);

        let f = posts.first().unwrap();
        assert_ne!(f.payload, None);
        assert_eq!(f.path, "/api/v1/login");
    }
}
