package steps;

import org.jbehave.core.annotations.Given;

public class PercentSteps {
    @Given("%customer holds %amount in wallet")
    public void customerHoldsInWallet(String customer, int amount) {}
}