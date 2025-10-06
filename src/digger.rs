/// Node is a struct that represents a single property in JSON data.
/// So this has a name and a possible value or a list of children, for example when you
/// have a nested object.
#[derive(Debug)]
pub struct Node {
    pub name: String,
    pub value: serde_json::Value,
    pub parent: Option<std::rc::Weak<std::cell::RefCell<Node>>>,
    pub children: Vec<std::rc::Rc<std::cell::RefCell<Node>>>,
}

impl Node {
    pub fn new(name: &str, value: serde_json::Value) -> std::rc::Rc<std::cell::RefCell<Self>> {
        std::rc::Rc::new(std::cell::RefCell::new(Self {
            name: name.to_string(),
            value,
            parent: None,
            children: Vec::new(),
        }))
    }

    /// Add a child node to the current node.
    fn add_child(
        parent: &std::rc::Rc<std::cell::RefCell<Self>>,
        child: std::rc::Rc<std::cell::RefCell<Self>>,
    ) {
        child.borrow_mut().parent = Some(std::rc::Rc::downgrade(parent));
        parent.borrow_mut().children.push(child);
    }
}

/// Diggere is a struct that holds the current state while populating the tree going through
/// the JSON data of all components.
pub struct Digger {
    pub root: std::rc::Rc<std::cell::RefCell<Node>>,
    current: std::rc::Rc<std::cell::RefCell<Node>>,
}

impl Digger {
    pub fn new() -> Self {
        let root = Node::new("root", serde_json::Value::Null);

        Self {
            root: std::rc::Rc::clone(&root),
            current: root,
        }
    }

    fn add_child_and_enter(&mut self, child_name: &str) {
        let child = Node::new(child_name, serde_json::Value::Null);
        Node::add_child(&self.current, std::rc::Rc::clone(&child));
        self.current = child;
    }

    fn exit_one_level(&mut self) {
        let parent = self
            .current
            .borrow()
            .parent
            .as_ref()
            .unwrap()
            .upgrade()
            .unwrap();
        self.current = parent;
    }

    pub fn dig(
        &mut self,
        schema: openapiv3::Schema,
        components: &openapiv3::Components,
    ) -> Result<(), String> {
        let openapiv3::SchemaKind::Type(t) = schema.schema_kind else {
            tracing::warn!("Unsupported schema kind: {:?}", schema.schema_kind);
            return Err("Unsupported schema kind".to_owned());
        };

        let openapiv3::Type::Object(obj) = t else {
            tracing::warn!("Unsupported type: {:?}", t);
            return Err("Unsupported type".to_owned());
        };

        for (name, p) in obj.properties {
            match p {
                openapiv3::ReferenceOr::Item(item) => {
                    let Some(v) = item.schema_data.example else {
                        let msg = format!("No example found for property: {name}");
                        return Err(msg);
                    };

                    let n = Node::new(&name, v);
                    n.borrow_mut().parent = Some(std::rc::Rc::downgrade(&self.current));

                    self.current.borrow_mut().children.push(n);
                }
                openapiv3::ReferenceOr::Reference { reference } => {
                    let (_, schema) = reference_to_schema_and_name(&reference, components)?;

                    self.add_child_and_enter(&name);

                    self.dig(schema, components)?;

                    self.exit_one_level();
                }
            }
        }

        Ok(())
    }
}

/// It converts a reference from full name to a tuple with schema name and schema.
fn reference_to_schema_and_name(
    reference: &str,
    components: &openapiv3::Components,
) -> Result<(String, openapiv3::Schema), String> {
    let name = reference.trim_start_matches("#/components/schemas/");
    let Some(schema) = components.schemas.get(name) else {
        let msg = format!("No schema found for reference: {reference}");
        return Err(msg);
    };

    let s = schema.as_item();
    let s = s.unwrap();

    Ok((name.to_owned(), s.clone()))
}

#[cfg(test)]
pub fn load_flat_level() -> std::rc::Rc<std::cell::RefCell<Node>> {
    let s = std::include_str!("./testdata/post_login.yml");
    let openapi_schema = serde_yaml_bw::from_str(&s);
    let openapi_schema: openapiv3::OpenAPI = openapi_schema.unwrap();
    let components = openapi_schema.components.unwrap();
    let posts = crate::collector::collect_post(&openapi_schema.paths, &components);

    let mut digger = Digger::new();
    let f = posts.first().unwrap();
    let s = f.payload.clone();
    let s = s.unwrap();

    let result = digger.dig(s, &components);
    assert!(result.is_ok());

    digger.root
}

#[cfg(test)]
pub fn load_nested() -> std::rc::Rc<std::cell::RefCell<Node>> {
    let s = std::include_str!("./testdata/post_info_nested_property.yml");
    let openapi_schema = serde_yaml_bw::from_str(&s);
    let openapi_schema: openapiv3::OpenAPI = openapi_schema.unwrap();
    let components = openapi_schema.components.unwrap();
    let posts = crate::collector::collect_post(&openapi_schema.paths, &components);

    let mut digger = Digger::new();
    let f = posts.first().unwrap();
    let s = f.payload.clone();
    let s = s.unwrap();

    let result = digger.dig(s, &components);
    assert!(result.is_ok());

    digger.root
}

#[cfg(test)]
pub fn load_nested_2() -> std::rc::Rc<std::cell::RefCell<Node>> {
    let s = std::include_str!("./testdata/post_info_nested_property_2.yml");
    let openapi_schema = serde_yaml_bw::from_str(&s);
    let openapi_schema: openapiv3::OpenAPI = openapi_schema.unwrap();
    let components = openapi_schema.components.unwrap();
    let posts = crate::collector::collect_post(&openapi_schema.paths, &components);

    let mut digger = Digger::new();
    let f = posts.first().unwrap();
    let s = f.payload.clone();
    let s = s.unwrap();

    let result = digger.dig(s, &components);
    assert!(result.is_ok());

    digger.root
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn nested() {
        let s = std::include_str!("./testdata/post_info_nested_property.yml");
        let openapi_schema: openapiv3::OpenAPI = serde_yaml_bw::from_str(s).unwrap();
        let components = openapi_schema.components.unwrap();
        let posts = crate::collector::collect_post(&openapi_schema.paths, &components);

        let f = posts.first().unwrap();
        assert_ne!(f.payload, None);

        let s = f.payload.clone();
        let s = s.unwrap();

        let mut digger = Digger::new();
        let result = digger.dig(s, &components);
        assert!(result.is_ok());

        // check the tree generated from the schema
        let root = digger.root.borrow();
        assert_eq!(root.children.len(), 1);

        let child = root.children.first().unwrap();
        assert_eq!(child.borrow().name, "hq");

        let hq = child.borrow();
        assert_eq!(hq.children.len(), 5);

        let mut children = hq.children.iter();
        let child = children.next().unwrap();
        assert_eq!(child.borrow().name, "address");

        let hq = children.next().unwrap();
        assert_eq!(hq.borrow().name, "postal_code");

        let child = children.next().unwrap();
        assert_eq!(child.borrow().name, "city");

        let child = children.next().unwrap();
        assert_eq!(child.borrow().name, "state_region");

        let child = children.next().unwrap();
        assert_eq!(child.borrow().name, "country");

        assert!(children.next().is_none());
    }

    #[test]
    fn nested_with_simple_along() {
        let s = std::include_str!("./testdata/post_info_nested_property_2.yml");
        let openapi_schema: openapiv3::OpenAPI = serde_yaml_bw::from_str(s).unwrap();
        let components = openapi_schema.components.unwrap();
        let posts = crate::collector::collect_post(&openapi_schema.paths, &components);

        let f = posts.first().unwrap();
        assert_ne!(f.payload, None);

        let s = f.payload.clone();
        let s = s.unwrap();

        let mut digger = Digger::new();
        let result = digger.dig(s, &components);
        assert!(result.is_ok());

        let root = digger.root.borrow();
        assert_eq!(root.children.len(), 2);

        let hq = root.children.first().unwrap();
        assert_eq!(hq.borrow().name, "hq");

        let plan_property = root.children.last().unwrap();
        assert_eq!(plan_property.borrow().name, "other");

        let hq = hq.borrow();
        assert_eq!(hq.children.len(), 5);

        let mut children = hq.children.iter();
        let child = children.next().unwrap();
        assert_eq!(child.borrow().name, "address");

        let child = children.next().unwrap();
        assert_eq!(child.borrow().name, "postal_code");

        let child = children.next().unwrap();
        assert_eq!(child.borrow().name, "city");

        let child = children.next().unwrap();
        assert_eq!(child.borrow().name, "state_region");

        let child = children.next().unwrap();
        assert_eq!(child.borrow().name, "country");

        assert!(children.next().is_none());
    }
}
