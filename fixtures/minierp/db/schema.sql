CREATE TABLE customer (
    id INTEGER PRIMARY KEY,
    customer_group INTEGER NOT NULL
);

CREATE TABLE approval_log (
    id INTEGER PRIMARY KEY,
    order_id INTEGER NOT NULL
);

SELECT id FROM customer WHERE customer_group = 7;
