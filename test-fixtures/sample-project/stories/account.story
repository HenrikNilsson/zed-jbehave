Narrative:
In order to manage my bank account
As a customer
I want to see my balance and make withdrawals

Meta:
@Feature account

Scenario: A customer checks their balance
Given Bob has 100 in account
When Bob withdraws 50
Then balance is 50

Scenario: Withdrawals can use aliases
Given Bob has 200 in account
When Bob takes out 50
Then the balance equals 150

Scenario: Undefined steps get flagged
Given a totally unknown precondition
Then a result that nobody defined
