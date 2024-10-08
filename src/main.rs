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

    println!("file lines count: {}", s.lines().count());

    let openapi_schema: openapiv3::OpenAPI = serde_yaml::from_str(&s).unwrap();
    println!("openapi version: {}", openapi_schema.openapi);

    openapi_schema.servers.iter().for_each(|s| {
        println!("server: {:?}", s.url);
    });

    let components = openapi_schema.components.unwrap();

    for (path, path_item) in openapi_schema.paths {
        println!("path: {}", path);

        let item = if let Some(item) = path_item.as_item() {
            item.to_owned()
        } else {
            continue;
        };

        if let Some(post) = item.post {
            println!("  post: {:?}", post.operation_id);
            println!("  request_body: {:?}", post.request_body);
            if let Some(request_body) = post.request_body {
                let req = if let Some(item) = request_body.as_item() {
                    item.to_owned()
                } else {
                    continue;
                };

                let content = req.content;
                for (c, media_type) in content {
                    println!("    content: {:?}", c);
                    println!("    media_type: {:?}", media_type);

                    let schema = media_type.schema.unwrap();
                    let reference = match schema {
                        openapiv3::ReferenceOr::Reference { reference } => {
                            println!("    reference: {:?}", reference);

                            reference
                        }
                        openapiv3::ReferenceOr::Item(i) => {
                            println!("    item: {:?}", i);
                            "".to_owned()
                        }
                    };

                    if reference != "" {
                        let reference = reference.trim_start_matches("#/components/schemas/");
                        let schema = components.schemas.get(reference).unwrap(); // TODO: handle the error
                        let prop = property_for_schema(schema.as_item().unwrap());
                        send_request(&openapi_schema.servers[0], &path, prop).await;
                    }
                }
            }
        }
    }
}

async fn send_request(server: &openapiv3::Server, path: &str, properties: std::collections::HashMap<String,openapiv3::ReferenceOr<Box<openapiv3::Schema>>>) {
    let url = format!("{}{}", server.url.to_owned()     , path);
    let client = reqwest::Client::new();


    let mut paylaod = std::collections::HashMap::new();
    for (name, prop) in properties {
        let item = prop.as_item().unwrap();
        println!("item: {:#?}", item.schema_data);
        let example = item.schema_data.example.clone().unwrap(); // TODO: handle the error
        paylaod.insert(name, example);
    }
    
    let s = serde_json::to_string(&paylaod).unwrap(); // TODO: handle the error
    println!("payload: {:?}", s);   
    let req = client.request(reqwest::Method::POST, &url).body(s).header("Content-Type", "application/json");
    let r = req.build().unwrap(); // TODO: handle the error
    println!("request: {:?}", r);
    let res = client.execute(r).await.unwrap(); // TODO: handle the error

    println!("response: {:?}", res);
}

fn property_for_schema(s: &openapiv3::  Schema) -> std::collections::HashMap<String,openapiv3::ReferenceOr<Box<openapiv3::Schema>>> {
    let mut properties = std::collections::HashMap::new();

    match &s.schema_kind {
        openapiv3::SchemaKind::Type(t) => {
            match t {
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
            }
        },
        _ => {}
    }

    properties
}
