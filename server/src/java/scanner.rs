use super::RawStepDefinition;
use super::common::scan_with_language;

pub fn scan(source: &str) -> Vec<RawStepDefinition> {
    scan_with_language(source, tree_sitter_java::LANGUAGE.into())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn finds_simple_given_step() {
        let source = r#"
            public class AccountSteps {
                @Given("$customer has $amount in account")
                public void hasBalance(String customer, int amount) {}
            }
        "#;
        let defs = scan(source);
        assert_eq!(defs.len(), 1);
        assert_eq!(defs[0].annotation, "Given");
        assert_eq!(defs[0].declaring_class, "AccountSteps");
        assert_eq!(defs[0].method_name, "hasBalance");
        assert_eq!(defs[0].patterns, vec!["$customer has $amount in account"]);
    }

    #[test]
    fn finds_aliases_on_the_same_method() {
        let source = r#"
            public class AccountSteps {
                @When("$customer withdraws $amount")
                @Alias("$customer takes out $amount")
                public void withdraw(String customer, int amount) {}
            }
        "#;
        let defs = scan(source);
        assert_eq!(defs.len(), 2);
        assert!(defs.iter().any(|d| d.annotation == "When"));
        assert!(defs.iter().any(|d| d.annotation == "Alias"));
        assert!(defs.iter().all(|d| d.method_name == "withdraw"));
    }

    #[test]
    fn finds_multiple_aliases_in_an_array() {
        let source = r#"
            public class AccountSteps {
                @Then("balance should be $balance")
                @Aliases({"balance is $balance", "the balance equals $balance"})
                public void checkBalance(int balance) {}
            }
        "#;
        let defs = scan(source);
        let aliases = defs.iter().find(|d| d.annotation == "Aliases").unwrap();
        assert_eq!(
            aliases.patterns,
            vec!["balance is $balance", "the balance equals $balance"]
        );
    }

    #[test]
    fn ignores_non_step_annotations() {
        let source = r#"
            public class AccountSteps {
                @Override
                @Deprecated
                public void notAStep() {}
            }
        "#;
        assert!(scan(source).is_empty());
    }

    #[test]
    fn tracks_declaring_class_across_multiple_methods() {
        let source = r#"
            public class AccountSteps {
                @Given("a")
                public void a() {}

                @When("b")
                public void b() {}
            }
        "#;
        let defs = scan(source);
        assert_eq!(defs.len(), 2);
        assert!(defs.iter().all(|d| d.declaring_class == "AccountSteps"));
    }
}
