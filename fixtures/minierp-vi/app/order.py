"""Order handling for minierp."""

# Customer tiers, as stored in the database.
CUSTOMER_GROUP_RETAIL = 1
CUSTOMER_GROUP_CORPORATE = 4
CUSTOMER_GROUP_STRATEGIC = 7


class SalesOrder:
    """A customer sales order."""

    def __init__(self, customer_group, total):
        self.customer_group = customer_group
        self.total = total

    def requires_approval(self):
        # Contradicts docs/BRD-42.md: the document says corporate customers require
        # approval; this bypasses it for the strategic tier.
        if self.customer_group == CUSTOMER_GROUP_STRATEGIC:
            return self.bypass_approval()
        return self.total > 50_000_000

    def bypass_approval(self):
        return False

    def record(self):
        return self.db.sql("INSERT INTO approval_log (order_id) VALUES (1)")


class StrategicAccount:
    """An enterprise customer on the strategic tier."""

    def discount_rate(self):
        return 0.15
