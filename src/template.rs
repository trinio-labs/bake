use std::{collections::BTreeMap, env};

use anyhow::bail;
use handlebars::Handlebars;
use indexmap::IndexMap;
use serde_json::json;

pub fn parse_template(
    template: &str,
    environment: &[String],
    variables: &IndexMap<String, String>,
    constants: &IndexMap<String, IndexMap<String, String>>,
) -> anyhow::Result<String> {
    // Get environment variables list from environment list
    let env_values: BTreeMap<String, String> = environment
        .iter()
        .map(|name| (name.to_string(), env::var(name).unwrap_or_default()))
        .collect();

    let mut handlebars = Handlebars::new();
    handlebars
        .register_template_string("template", template)
        .expect("Failed to register template");

    let mut data = BTreeMap::from([("env", json!(env_values)), ("var", json!(variables))]);
    data.extend(constants.iter().map(|(k, v)| (k.as_ref(), json!(v))));

    match handlebars.render("template", &data) {
        Ok(rendered) => Ok(rendered),
        Err(err) => bail!("Failed to render template: {}", err),
    }
}

pub fn parse_variable_list(
    environment: &[String],
    variables: &IndexMap<String, String>,
    constants: &IndexMap<String, IndexMap<String, String>>,
    override_variables: &IndexMap<String, String>,
) -> anyhow::Result<IndexMap<String, String>> {
    variables
        .iter()
        .try_fold(IndexMap::new(), |mut acc, (k, v)| {
            if override_variables.contains_key(k) {
                acc.insert(k.clone(), override_variables[k].clone());
                return Ok(acc);
            }
            let parsed_var = parse_template(v, environment, &acc, constants)?;
            acc.insert(k.clone(), parsed_var);
            Ok(acc)
        })
}

#[cfg(test)]
mod test {
    use super::*;

    #[test]
    fn test_parse_template() {
        let variables = IndexMap::from([("foo".to_owned(), "bar".to_owned())]);
        let constants = IndexMap::from([(
            "project".to_owned(),
            IndexMap::from([("foo".to_owned(), "bar".to_owned())]),
        )]);

        let environment = vec!["TEST_PARSE_TEMPLATE".to_owned()];
        env::set_var("TEST_PARSE_TEMPLATE", "env_var");

        let result = parse_template(
            "{{var.foo}}",
            environment.as_slice(),
            &variables,
            &constants,
        )
        .unwrap();
        assert_eq!(result, "bar");

        let result = parse_template(
            "{{project.foo}}",
            environment.as_slice(),
            &variables,
            &constants,
        )
        .unwrap();
        assert_eq!(result, "bar");

        let result = parse_template(
            "{{env.TEST_PARSE_TEMPLATE}}",
            environment.as_slice(),
            &variables,
            &constants,
        )
        .unwrap();
        assert_eq!(result, "env_var");
    }

    #[test]
    fn test_parse_variable_list() {
        let environment = vec!["TEST_PARSE_VARIABLE_LIST".to_owned()];
        env::set_var("TEST_PARSE_VARIABLE_LIST", "bar");

        let constants = IndexMap::from([(
            "project".to_owned(),
            IndexMap::from([("foo".to_owned(), "bar".to_owned())]),
        )]);

        let overrides = IndexMap::from([("bar".to_owned(), "override".to_owned())]);

        let variables = IndexMap::from([
            (
                "foo".to_owned(),
                "{{env.TEST_PARSE_VARIABLE_LIST}}".to_owned(),
            ),
            ("baz".to_owned(), "{{var.foo}}".to_owned()),
            ("bar".to_owned(), "bar".to_owned()),
            ("goo".to_owned(), "{{ var.bar }}".to_owned()),
        ]);

        let result =
            parse_variable_list(environment.as_slice(), &variables, &constants, &overrides)
                .unwrap();
        assert_eq!(result.get("foo").unwrap(), "bar");
        assert_eq!(result.get("baz").unwrap(), "bar");
        assert_eq!(result.get("bar").unwrap(), "override");
        assert_eq!(result.get("goo").unwrap(), "override");
    }
}
