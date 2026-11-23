# SPDX-License-Identifier: Apache-2.0
"""A two-ledger bank account with an append-only audit log."""

REJECTED = {"ok": False, "kind": "requires_failed"}
LOG_CAPACITY = 4


class BankSystem:
    def reset(self):
        self.cleared = 0
        self.pending = 0
        self.audit_balance = 0
        self.audit_log = []
        self.withdrawn = 0

    def deposit(self, amount):
        if amount <= 0 or len(self.audit_log) >= LOG_CAPACITY:
            return REJECTED
        self.pending += amount
        self.audit_balance += amount
        self.audit_log.append(amount)
        return {"ok": True}

    def settle(self):
        if self.pending <= 0:
            return REJECTED
        self.cleared += self.pending
        self.pending = 0
        return {"ok": True}

    def withdraw(self, amount):
        if amount <= 0 or self.cleared < amount:
            return REJECTED
        self.cleared -= amount
        self.withdrawn += amount
        return {"ok": True}


class Conforming:
    """Adapter over BankSystem; state names are the compose root's public ones."""

    implementation = BankSystem

    def __init__(self):
        self.bank = self.implementation()

    def reset(self):
        self.bank.reset()

    def step(self, action, params):
        if action == "deposit_audited":
            return self.bank.deposit(params["a"])
        if action == "bank.settle":
            return self.bank.settle()
        if action == "withdraw_audited":
            return self.bank.withdraw(params["a"])
        raise ValueError(f"unknown action {action}")

    def observe(self):
        return {
            "bank.cleared": self.bank.cleared,
            "bank.pending": self.bank.pending,
            "audit.balance": self.bank.audit_balance,
            "audit.log": list(self.bank.audit_log),
            "withdrawn": self.bank.withdrawn,
        }


class DoubleCountingBank(BankSystem):
    """The defect: settling clears the funds but leaves them pending as well."""

    def settle(self):
        if self.pending <= 0:
            return REJECTED
        self.cleared += self.pending
        return {"ok": True}


class NonConforming(Conforming):
    implementation = DoubleCountingBank
