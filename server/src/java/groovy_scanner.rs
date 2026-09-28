use super::RawStepDefinition;
use super::common::scan_with_language;

pub fn scan(source: &str) -> Vec<RawStepDefinition> {
    scan_with_language(source, tree_sitter_groovy::LANGUAGE.into())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn finds_simple_given_step() {
        let source = r#"
            class AccountSteps {
                @Given("$customer has $amount in account")
                void hasBalance(String customer, int amount) {}
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
            class AccountSteps {
                @When("$customer withdraws $amount")
                @Alias("$customer takes out $amount")
                void withdraw(String customer, int amount) {}
            }
        "#;
        let defs = scan(source);
        assert_eq!(defs.len(), 2);
        assert!(defs.iter().any(|d| d.annotation == "When"));
        assert!(defs.iter().any(|d| d.annotation == "Alias"));
        assert!(defs.iter().all(|d| d.method_name == "withdraw"));
    }

    #[test]
    fn groovy_def_methods_are_found_too() {
        let source = r#"
            class AccountSteps {
                @Then("balance should be $balance")
                def checkBalance(int balance) {}
            }
        "#;
        let defs = scan(source);
        assert_eq!(defs.len(), 1);
        assert_eq!(defs[0].method_name, "checkBalance");
    }
}
