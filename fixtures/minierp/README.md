# minierp

A deliberately tiny business system used as a test fixture.

Every claim Reify makes about this repository has a known right answer, which is what
makes it useful: the knowledge here is *planted*, and the tests assert it is found.

| Planted | Where | Expected |
|---|---|---|
| A documented approval rule | `docs/BRD-42.md` | mined as a `Document` rule |
| Code that contradicts it | `app/order.py` | mined as a `CodeGuard` rule, opposite polarity |
| A magic number for a customer tier | `app/order.py`, `db/schema.sql` | `CUSTOMER_GROUP = 7` reachable from the concept |
| A bilingual concept | `i18n/vi.csv` | `STRATEGIC_ACCOUNT` with `eng` and `vie` labels |
| Cross-module data coupling | `app/report.py` | affected by `order.py` through a shared table, with no call edge |
| A rule stated only by a test | `app/test_rules.py` | mined as a `Test` rule |
