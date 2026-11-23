# SPDX-License-Identifier: Apache-2.0
"""Order lifecycle: Cart -> Paid -> Shipped, and Paid -> Cancelled."""

REJECTED = {"ok": False, "kind": "requires_failed"}


class Orders:
    cancellable = ("Paid",)

    def reset(self):
        self.order = {0: "Cart", 1: "Cart"}

    def move(self, order, allowed, to):
        if self.order[order] not in allowed:
            return REJECTED
        self.order[order] = to
        return {"ok": True}

    def pay(self, order):
        return self.move(order, ("Cart",), "Paid")

    def ship(self, order):
        return self.move(order, ("Paid",), "Shipped")

    def cancel(self, order):
        return self.move(order, self.cancellable, "Cancelled")


class Conforming:
    implementation = Orders

    def __init__(self):
        self.orders = self.implementation()

    def reset(self):
        self.orders.reset()

    def step(self, action, params):
        if action in ("pay", "ship", "cancel"):
            return getattr(self.orders, action)(params["o"])
        raise ValueError(f"unknown action {action}")

    def observe(self):
        return {"order": {str(order): status for order, status in self.orders.order.items()}}


class CancelsAfterShipping(Orders):
    """The defect: a shipped order can still be cancelled."""

    cancellable = ("Paid", "Shipped")


class NonConforming(Conforming):
    implementation = CancelsAfterShipping
