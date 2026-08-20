from app.order import SalesOrder


class NightlyApprovalJob:
    def go(self, orders):
        return [o.requires_approval() for o in orders]
