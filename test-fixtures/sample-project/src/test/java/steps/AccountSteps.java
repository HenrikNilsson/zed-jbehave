package steps;

import org.jbehave.core.annotations.Alias;
import org.jbehave.core.annotations.Aliases;
import org.jbehave.core.annotations.Given;
import org.jbehave.core.annotations.Then;
import org.jbehave.core.annotations.When;

/**
 * Fixture step definitions used by test-fixtures/sample-project/stories/
 * account.story (see server/tests/definition.rs, which drives the language
 * server against both) and, in the meantime, by
 * server/src/java/scanner.rs's own unit tests as a reference for what a
 * realistic steps class looks like.
 */
public class AccountSteps {

    @Given("$customer has $amount in account")
    public void hasBalance(String customer, int amount) {
    }

    @When("$customer withdraws $amount")
    @Alias("$customer takes out $amount")
    public void withdraw(String customer, int amount) {
    }

    @Then("balance should be $balance")
    @Aliases({"balance is $balance", "the balance equals $balance"})
    public void checkBalance(int balance) {
    }

    // Deliberately never referenced by any .story fixture, once those exist:
    // exercises the "unused step declaration" inspection.
    @Given("the system is in maintenance mode")
    public void maintenanceMode() {
    }
}
