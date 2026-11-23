# SPDX-License-Identifier: Apache-2.0
"""A shopping cart: two users, two items, one of each item in stock."""

REJECTED = {"ok": False, "kind": "requires_failed"}


class Cart:
    def reset(self):
        self.stock = {0: 1, 1: 1}
        self.cart = {0: None, 1: None}

    def add_to_cart(self, user, item):
        if self.cart[user] is not None:
            return REJECTED
        self.cart[user] = item
        return {"ok": True}

    def remove_from_cart(self, user):
        if self.cart[user] is None:
            return REJECTED
        self.cart[user] = None
        return {"ok": True}

    def checkout(self, user):
        item = self.cart[user]
        if item is None or self.stock[item] <= 0:
            return REJECTED
        self.stock[item] -= 1
        self.cart[user] = None
        return {"ok": True}


class Conforming:
    """Adapter over Cart, projected onto the spec's state shape."""

    implementation = Cart

    def __init__(self):
        self.cart = self.implementation()

    def reset(self):
        self.cart.reset()

    def step(self, action, params):
        if action == "add_to_cart":
            return self.cart.add_to_cart(params["u"], params["i"])
        if action == "remove_from_cart":
            return self.cart.remove_from_cart(params["u"])
        if action == "checkout":
            return self.cart.checkout(params["u"])
        raise ValueError(f"unknown action {action}")

    def observe(self):
        return {
            "stock": {str(item): quantity for item, quantity in self.cart.stock.items()},
            "cart": {str(user): item for user, item in self.cart.cart.items()},
        }


class OversellingCart(Cart):
    """The defect: checkout empties the cart but never takes the item from stock."""

    def checkout(self, user):
        if self.cart[user] is None:
            return REJECTED
        self.cart[user] = None
        return {"ok": True}


class NonConforming(Conforming):
    implementation = OversellingCart
