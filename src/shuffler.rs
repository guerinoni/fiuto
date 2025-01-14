pub fn do_it(
    point: &std::rc::Rc<std::cell::RefCell<crate::digger::Node>>,
) -> Vec<std::collections::HashMap<String, serde_json::Value>> {
    let mut properties = vec![];

    let mut sub_properties = std::collections::HashMap::new();

    // this check if all kids are leaves, becuse if only one has children, we need to go deeper.
    // otherwise, we can generate the combinations.
    for c in &point.borrow().children {
        let n = c.borrow();
        properties.push((n.name.clone(), n.value.clone()));

        if !n.children.is_empty() {
            let m = do_it(c);
            sub_properties.insert(c.borrow().name.clone(), m);
            continue;
        }
    }

    let mut combs = vec![];

    let total_combinations = (1 << properties.len()) - 1;
    for mask in 1..=total_combinations {
        let mut c = std::collections::HashMap::new();
        for (i, p) in properties.iter().enumerate() {
            if (mask & (1 << i)) == 0 {
                continue;
            }

            c.insert(p.0.clone(), p.1.clone());
        }

        combs.push(c);
    }

    let mut to_push = vec![];

    // for every combination, we need to add the sub properties to it.
    // if you have for example:
    // - string
    // - object
    // you will have
    // - [string]
    // - [null]
    // - [object (with all possible combinations)]
    // [string, object (with all possible combinations)]
    for c in &combs {
        for (k, v) in &sub_properties {
            v.iter()
                .map(|variant| serde_json::to_value(variant).unwrap())
                .for_each(|vv| {
                    let mut h = c.clone();
                    h.insert(k.clone(), vv);
                    to_push.push(h);
                });
        }
    }

    for t in to_push {
        combs.push(t);
    }

    combs
}

#[cfg(test)]
mod tests {
    #[test]
    fn one_level_properties() {
        // here we have:
        // - email
        // - org
        // - password
        let root = crate::digger::load_flat_level();
        let c = crate::shuffler::do_it(&root);

        println!("{:#?}", c);

        assert_eq!(c.len(), 7);

        let zero = c.get(0).unwrap();
        assert!(zero.contains_key("email"));

        let one = c.get(1).unwrap();
        assert!(one.contains_key("org"));

        let two = c.get(2).unwrap();
        assert!(two.contains_key("email"));
        assert!(two.contains_key("org"));

        let three = c.get(3).unwrap();
        assert!(three.contains_key("password"));

        let four = c.get(4).unwrap();
        assert!(four.contains_key("email"));
        assert!(four.contains_key("password"));

        let five = c.get(5).unwrap();
        assert!(five.contains_key("org"));
        assert!(five.contains_key("password"));

        let six = c.get(6).unwrap();
        assert!(six.contains_key("email"));
        assert!(six.contains_key("org"));
        assert!(six.contains_key("password"));
    }

    #[test]
    fn one_as_object() {
        // here we have:
        // - hq -> address, postal_code, city, state_region, country

        let root = crate::digger::load_nested();
        let c = crate::shuffler::do_it(&root);

        println!("{:#?}", c);

        assert_eq!(c.len(), 32);

        let zero = c.get(0).unwrap();
        assert!(zero.contains_key("hq"));
        assert_eq!(zero.get("hq").unwrap(), &serde_json::Value::Null);

        let one = c.get(1).unwrap();
        let hq = one.get("hq").unwrap().as_object().unwrap();
        assert!(hq.contains_key("address"));

        let two = c.get(2).unwrap();
        let hq = two.get("hq").unwrap().as_object().unwrap();
        assert!(hq.contains_key("postal_code"));

        let three = c.get(3).unwrap();
        let hq = three.get("hq").unwrap().as_object().unwrap();
        assert!(hq.contains_key("address"));
        assert!(hq.contains_key("postal_code"));

        let four = c.get(4).unwrap();
        let hq = four.get("hq").unwrap().as_object().unwrap();
        assert!(hq.contains_key("city"));

        let five = c.get(5).unwrap();
        let hq = five.get("hq").unwrap().as_object().unwrap();
        assert!(hq.contains_key("address"));
        assert!(hq.contains_key("city"));

        let six = c.get(6).unwrap();
        let hq = six.get("hq").unwrap().as_object().unwrap();
        assert!(hq.contains_key("postal_code"));
        assert!(hq.contains_key("city"));

        let seven = c.get(7).unwrap();
        let hq = seven.get("hq").unwrap().as_object().unwrap();
        assert!(hq.contains_key("address"));
        assert!(hq.contains_key("postal_code"));
        assert!(hq.contains_key("city"));

        let eight = c.get(8).unwrap();
        let hq = eight.get("hq").unwrap().as_object().unwrap();
        assert!(hq.contains_key("state_region"));

        let nine = c.get(9).unwrap();
        let hq = nine.get("hq").unwrap().as_object().unwrap();
        assert!(hq.contains_key("address"));
        assert!(hq.contains_key("state_region"));

        let ten = c.get(10).unwrap();
        let hq = ten.get("hq").unwrap().as_object().unwrap();
        assert!(hq.contains_key("postal_code"));
        assert!(hq.contains_key("state_region"));

        let eleven = c.get(11).unwrap();
        let hq = eleven.get("hq").unwrap().as_object().unwrap();
        assert!(hq.contains_key("address"));
        assert!(hq.contains_key("postal_code"));
        assert!(hq.contains_key("state_region"));

        let twelve = c.get(12).unwrap();
        let hq = twelve.get("hq").unwrap().as_object().unwrap();
        assert!(hq.contains_key("city"));
        assert!(hq.contains_key("state_region"));

        let thirteen = c.get(13).unwrap();
        let hq = thirteen.get("hq").unwrap().as_object().unwrap();
        assert!(hq.contains_key("address"));
        assert!(hq.contains_key("city"));
        assert!(hq.contains_key("state_region"));

        let fourteen = c.get(14).unwrap();
        let hq = fourteen.get("hq").unwrap().as_object().unwrap();
        assert!(hq.contains_key("postal_code"));
        assert!(hq.contains_key("city"));
        assert!(hq.contains_key("state_region"));

        let fifteen = c.get(15).unwrap();
        let hq = fifteen.get("hq").unwrap().as_object().unwrap();
        assert!(hq.contains_key("address"));
        assert!(hq.contains_key("postal_code"));
        assert!(hq.contains_key("city"));
        assert!(hq.contains_key("state_region"));

        let sixteen = c.get(16).unwrap();
        let hq = sixteen.get("hq").unwrap().as_object().unwrap();
        assert!(hq.contains_key("country"));

        let seventeen = c.get(17).unwrap();
        let hq = seventeen.get("hq").unwrap().as_object().unwrap();
        assert!(hq.contains_key("address"));
        assert!(hq.contains_key("country"));

        let eighteen = c.get(18).unwrap();
        let hq = eighteen.get("hq").unwrap().as_object().unwrap();
        assert!(hq.contains_key("postal_code"));
        assert!(hq.contains_key("country"));

        let nineteen = c.get(19).unwrap();
        let hq = nineteen.get("hq").unwrap().as_object().unwrap();
        assert!(hq.contains_key("address"));
        assert!(hq.contains_key("postal_code"));
        assert!(hq.contains_key("country"));

        let twenty = c.get(20).unwrap();
        let hq = twenty.get("hq").unwrap().as_object().unwrap();
        assert!(hq.contains_key("city"));
        assert!(hq.contains_key("country"));

        let twenty_one = c.get(21).unwrap();
        let hq = twenty_one.get("hq").unwrap().as_object().unwrap();
        assert!(hq.contains_key("address"));
        assert!(hq.contains_key("city"));
        assert!(hq.contains_key("country"));

        let twenty_two = c.get(22).unwrap();
        let hq = twenty_two.get("hq").unwrap().as_object().unwrap();
        assert!(hq.contains_key("postal_code"));
        assert!(hq.contains_key("city"));
        assert!(hq.contains_key("country"));

        let twenty_three = c.get(23).unwrap();
        let hq = twenty_three.get("hq").unwrap().as_object().unwrap();
        assert!(hq.contains_key("address"));
        assert!(hq.contains_key("postal_code"));
        assert!(hq.contains_key("city"));
        assert!(hq.contains_key("country"));

        let twenty_four = c.get(24).unwrap();
        let hq = twenty_four.get("hq").unwrap().as_object().unwrap();
        assert!(hq.contains_key("state_region"));
        assert!(hq.contains_key("country"));

        let twenty_five = c.get(25).unwrap();
        let hq = twenty_five.get("hq").unwrap().as_object().unwrap();
        assert!(hq.contains_key("address"));
        assert!(hq.contains_key("state_region"));
        assert!(hq.contains_key("country"));

        let twenty_six = c.get(26).unwrap();
        let hq = twenty_six.get("hq").unwrap().as_object().unwrap();
        assert!(hq.contains_key("postal_code"));
        assert!(hq.contains_key("state_region"));
        assert!(hq.contains_key("country"));

        let twenty_seven = c.get(27).unwrap();
        let hq = twenty_seven.get("hq").unwrap().as_object().unwrap();
        assert!(hq.contains_key("address"));
        assert!(hq.contains_key("postal_code"));
        assert!(hq.contains_key("state_region"));
        assert!(hq.contains_key("country"));

        let twenty_eight = c.get(28).unwrap();
        let hq = twenty_eight.get("hq").unwrap().as_object().unwrap();
        assert!(hq.contains_key("city"));
        assert!(hq.contains_key("state_region"));
        assert!(hq.contains_key("country"));

        let twenty_nine = c.get(29).unwrap();
        let hq = twenty_nine.get("hq").unwrap().as_object().unwrap();
        assert!(hq.contains_key("address"));
        assert!(hq.contains_key("city"));
        assert!(hq.contains_key("state_region"));
        assert!(hq.contains_key("country"));

        let thirty = c.get(30).unwrap();
        let hq = thirty.get("hq").unwrap().as_object().unwrap();
        assert!(hq.contains_key("postal_code"));
        assert!(hq.contains_key("city"));
        assert!(hq.contains_key("state_region"));
        assert!(hq.contains_key("country"));

        let thirty_one = c.get(31).unwrap();
        let hq = thirty_one.get("hq").unwrap().as_object().unwrap();
        assert!(hq.contains_key("address"));
        assert!(hq.contains_key("postal_code"));
        assert!(hq.contains_key("city"));
        assert!(hq.contains_key("state_region"));
        assert!(hq.contains_key("country"));
    }

    #[test]
    fn one_string_and_one_as_object() {
        // here we have:
        // - other (string)
        // - hq -> address, postal_code, city, state_region, country

        let root = crate::digger::load_nested_2();
        let c = crate::shuffler::do_it(&root);

        println!("{:#?} - {}", c, c.len());

        assert_eq!(c.len(), 96);
    }
}
