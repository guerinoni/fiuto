/// Node is a struct that represents a single property in JSON data.
/// So this has a name and a possible value or a list of children, for example when you
/// have a nested object.
#[derive(Debug)]
pub struct Node {
    pub name: String,
    pub value: serde_json::Value,
    pub parent: Option<std::rc::Weak<std::cell::RefCell<Self>>>,
    pub children: Vec<std::rc::Rc<std::cell::RefCell<Self>>>,
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

    /// Walks the properties of an object schema, building the tree. A property
    /// that resolves to an object with its own properties becomes a nested
    /// level; any other property is a leaf and must carry an example value.
    pub fn dig(
        &mut self,
        schema: &oas3::spec::ObjectSchema,
        spec: &oas3::Spec,
    ) -> Result<(), String> {
        for (name, prop) in &schema.properties {
            let resolved = crate::collector::resolve_object_schema(prop, spec)?;

            if resolved.properties.is_empty() {
                let Some(v) = resolved.example.clone() else {
                    let msg = format!("No example found for property: {name}");
                    return Err(msg);
                };

                let n = Node::new(name, v);
                n.borrow_mut().parent = Some(std::rc::Rc::downgrade(&self.current));

                self.current.borrow_mut().children.push(n);
            } else {
                self.add_child_and_enter(name);

                self.dig(&resolved, spec)?;

                self.exit_one_level();
            }
        }

        Ok(())
    }
}

#[cfg(test)]
fn dig_payload(spec_yaml: &str) -> std::rc::Rc<std::cell::RefCell<Node>> {
    let spec = crate::parse_openapi(spec_yaml).unwrap();
    let posts = crate::collector::collect_post(&spec);

    let mut digger = Digger::new();
    let f = posts.first().unwrap();
    let payload = f.payload.clone().unwrap();

    let result = digger.dig(&payload, &spec);
    assert!(result.is_ok());

    digger.root
}

#[cfg(test)]
pub fn load_flat_level() -> std::rc::Rc<std::cell::RefCell<Node>> {
    dig_payload(std::include_str!("./testdata/post_login.yml"))
}

#[cfg(test)]
pub fn load_nested() -> std::rc::Rc<std::cell::RefCell<Node>> {
    dig_payload(std::include_str!(
        "./testdata/post_info_nested_property.yml"
    ))
}

#[cfg(test)]
pub fn load_nested_2() -> std::rc::Rc<std::cell::RefCell<Node>> {
    dig_payload(std::include_str!(
        "./testdata/post_info_nested_property_2.yml"
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn nested() {
        let s = std::include_str!("./testdata/post_info_nested_property.yml");
        let spec = crate::parse_openapi(s).unwrap();
        let posts = crate::collector::collect_post(&spec);

        let f = posts.first().unwrap();
        assert!(f.payload.is_some());

        let payload = f.payload.clone().unwrap();

        let mut digger = Digger::new();
        let result = digger.dig(&payload, &spec);
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
        let spec = crate::parse_openapi(s).unwrap();
        let posts = crate::collector::collect_post(&spec);

        let f = posts.first().unwrap();
        assert!(f.payload.is_some());

        let payload = f.payload.clone().unwrap();

        let mut digger = Digger::new();
        let result = digger.dig(&payload, &spec);
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

    #[test]
    fn node_creation_and_parent_child_relationship() {
        let parent = Node::new("parent", serde_json::json!("parent_value"));
        let child = Node::new("child", serde_json::json!("child_value"));

        Node::add_child(&parent, std::rc::Rc::clone(&child));

        // Check parent has the child
        assert_eq!(parent.borrow().children.len(), 1);
        assert_eq!(parent.borrow().children[0].borrow().name, "child");

        // Check child has reference to parent
        assert!(child.borrow().parent.is_some());
        let parent_ref = child.borrow().parent.as_ref().unwrap().upgrade().unwrap();
        assert_eq!(parent_ref.borrow().name, "parent");
    }

    #[test]
    fn digger_initializes_with_root() {
        let digger = Digger::new();
        assert_eq!(digger.root.borrow().name, "root");
        assert_eq!(digger.root.borrow().value, serde_json::Value::Null);
        assert_eq!(digger.root.borrow().children.len(), 0);
    }

    #[test]
    fn flat_properties_are_added_to_tree() {
        let root = load_flat_level();
        let root_borrowed = root.borrow();

        // Should have 3 children: email, org, password
        assert_eq!(root_borrowed.children.len(), 3);

        let mut child_names: Vec<String> = root_borrowed
            .children
            .iter()
            .map(|c| c.borrow().name.clone())
            .collect();
        child_names.sort();

        assert_eq!(child_names, vec!["email", "org", "password"]);
    }

    #[test]
    fn nested_properties_create_hierarchy() {
        let root = load_nested();
        let root_borrowed = root.borrow();

        // Should have 1 child: hq
        assert_eq!(root_borrowed.children.len(), 1);

        let hq = root_borrowed.children.first().unwrap();
        assert_eq!(hq.borrow().name, "hq");

        // hq should have 5 children
        assert_eq!(hq.borrow().children.len(), 5);
    }

    #[test]
    fn mixed_flat_and_nested_properties() {
        let root = load_nested_2();
        let root_borrowed = root.borrow();

        // Should have 2 children: hq (nested) and other (flat)
        assert_eq!(root_borrowed.children.len(), 2);

        let mut found_flat = false;
        let mut found_nested = false;

        for child in &root_borrowed.children {
            let child_borrowed = child.borrow();
            if child_borrowed.name == "other" {
                found_flat = true;
                // Flat properties should have no children
                assert_eq!(child_borrowed.children.len(), 0);
            } else if child_borrowed.name == "hq" {
                found_nested = true;
                // Nested object should have children
                assert!(child_borrowed.children.len() > 0);
            }
        }

        assert!(found_flat);
        assert!(found_nested);
    }
}
