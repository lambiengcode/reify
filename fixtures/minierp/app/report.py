"""Reporting. Coupled to order.py only through the database."""


class ApprovalReport:
    def run(self):
        return self.db.sql("SELECT count(*) FROM approval_log")
